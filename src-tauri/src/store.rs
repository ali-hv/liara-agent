use crate::models::*;
use rusqlite::{Connection,params};
use serde::{Serialize,de::DeserializeOwned};
use std::{path::{Path,PathBuf},sync::Mutex};

pub struct Store{conn:Mutex<Connection>,pub data_dir:PathBuf}
impl Store {
 pub fn new(path:&Path)->Result<Self>{
  std::fs::create_dir_all(path).map_err(|e|e.to_string())?;
  use std::os::unix::fs::PermissionsExt;std::fs::set_permissions(path,std::fs::Permissions::from_mode(0o700)).map_err(|e|e.to_string())?;
  let conn=Connection::open(path.join("launch.sqlite")).map_err(|e|e.to_string())?;
  conn.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE IF NOT EXISTS objects (kind TEXT NOT NULL,id TEXT NOT NULL,account TEXT NOT NULL DEFAULT '',json TEXT NOT NULL,updated TEXT NOT NULL,PRIMARY KEY(kind,id)); CREATE TABLE IF NOT EXISTS hidden_projects(account TEXT NOT NULL,path TEXT NOT NULL,PRIMARY KEY(account,path));").map_err(|e|e.to_string())?;
  Ok(Self{conn:Mutex::new(conn),data_dir:path.into()})
 }
 pub fn save<T:Serialize>(&self,kind:&str,id:&str,account:&str,value:&T)->Result<()>{let json=serde_json::to_string(value).map_err(|e|e.to_string())?;self.conn.lock().map_err(|_|"Store unavailable")?.execute("INSERT INTO objects(kind,id,account,json,updated) VALUES (?1,?2,?3,?4,?5) ON CONFLICT(kind,id) DO UPDATE SET account=excluded.account,json=excluded.json,updated=excluded.updated",params![kind,id,account,json,now()]).map_err(|e|e.to_string())?;Ok(())}
 pub fn get<T:DeserializeOwned>(&self,kind:&str,id:&str)->Result<T>{let s:String=self.conn.lock().map_err(|_|"Store unavailable")?.query_row("SELECT json FROM objects WHERE kind=?1 AND id=?2",params![kind,id],|r|r.get(0)).map_err(|_|"Saved item not found.")?;serde_json::from_str(&s).map_err(|e|e.to_string())}
 pub fn list<T:DeserializeOwned>(&self,kind:&str,account:&str)->Result<Vec<T>>{let conn=self.conn.lock().map_err(|_|"Store unavailable")?;let mut stmt=conn.prepare("SELECT json FROM objects WHERE kind=?1 AND account=?2 AND (?1 != 'project' OR NOT EXISTS (SELECT 1 FROM hidden_projects h WHERE h.account=objects.account AND h.path=json_extract(objects.json,'$.path'))) ORDER BY updated DESC LIMIT 100").map_err(|e|e.to_string())?;let rows=stmt.query_map(params![kind,account],|r|r.get::<_,String>(0)).map_err(|e|e.to_string())?;rows.map(|r|{let s=r.map_err(|e|e.to_string())?;serde_json::from_str(&s).map_err(|e|e.to_string())}).collect()}
 pub fn hide_project(&self,id:&str,account:&str)->Result<()>{
  let conn=self.conn.lock().map_err(|_|"Store unavailable")?;
  let changed=conn.execute("INSERT OR IGNORE INTO hidden_projects(account,path) SELECT account,json_extract(json,'$.path') FROM objects WHERE kind='project' AND id=?1 AND account=?2",params![id,account]).map_err(|e|e.to_string())?;
  if changed==0{return Err("Project is unavailable or already removed from recent projects.".into());}Ok(())
 }
 pub fn show_project(&self,path:&str,account:&str)->Result<()>{self.conn.lock().map_err(|_|"Store unavailable")?.execute("DELETE FROM hidden_projects WHERE account=?1 AND path=?2",params![account,path]).map_err(|e|e.to_string())?;Ok(())}
 pub fn delete_run(&self,id:&str,account:&str)->Result<()>{let changed=self.conn.lock().map_err(|_|"Store unavailable")?.execute("DELETE FROM objects WHERE kind='run' AND id=?1 AND account=?2",params![id,account]).map_err(|e|e.to_string())?;if changed==0{return Err("Deployment is unavailable for this account.".into());}Ok(())}
 pub fn stage_dir(&self,plan:&str,service:&str)->PathBuf{self.data_dir.join("staging").join(plan).join(service)}
}

#[cfg(test)]mod tests{
 use super::*;use serde_json::{json,Value};
 #[test]fn removing_recent_projects_preserves_history_and_is_account_scoped(){
  let dir=tempfile::tempdir().unwrap();let store=Store::new(dir.path()).unwrap();
  store.save("project","first","personal",&json!({"path":"/example/project"})).unwrap();
  store.save("project","second","personal",&json!({"path":"/example/project"})).unwrap();
  store.save("project","team-copy","team",&json!({"path":"/example/project"})).unwrap();
  assert!(store.hide_project("first","team").is_err());
  store.hide_project("first","personal").unwrap();assert!(store.list::<Value>("project","personal").unwrap().is_empty());
  assert_eq!(store.list::<Value>("project","team").unwrap().len(),1);
  assert_eq!(store.get::<Value>("project","first").unwrap()["path"],"/example/project");
  store.show_project("/example/project","personal").unwrap();assert_eq!(store.list::<Value>("project","personal").unwrap().len(),2);
 }
 #[test]fn history_deletion_only_removes_the_owned_run(){
  let dir=tempfile::tempdir().unwrap();let store=Store::new(dir.path()).unwrap();
  store.save("run","run-1","team",&json!({"plan":"plan-1"})).unwrap();store.save("plan","plan-1","team",&json!({"revision":"approved"})).unwrap();
  assert!(store.delete_run("run-1","personal").is_err());assert!(store.get::<Value>("run","run-1").is_ok());
  store.delete_run("run-1","team").unwrap();assert!(store.get::<Value>("run","run-1").is_err());assert!(store.get::<Value>("plan","plan-1").is_ok());
 }
}
