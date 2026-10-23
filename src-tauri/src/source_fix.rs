use crate::{models::*,project,security::*};
use serde::{Deserialize,Serialize};
use std::{collections::BTreeSet,fs,path::{Path,Component}};

#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct FileEdit {pub path:String,pub before:String,pub after:String,pub reason:String}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct FixAdvice {pub explanation:String,pub changes:Vec<FileEdit>}
#[derive(Clone,Debug,Serialize)]
#[serde(rename_all="camelCase")]
pub struct FixReview {pub id:String,pub plan_id:String,pub revision:String,pub explanation:String,pub changes:Vec<FileEdit>,pub expires_at:String}

pub fn infrastructure_error(message:&str)->bool{
 message.starts_with("HTTP ")||message.starts_with("Network request failed")||message.contains("not_enough_storage_space")
}
fn editable(root:&Path,relative:&str)->Result<std::path::PathBuf>{
 let rel=Path::new(relative);
 if relative.is_empty()||sensitive_path(rel)||rel.components().any(|c|!matches!(c,Component::Normal(_))){return Err("This file cannot be changed by the assistant.".into());}
 let mut path=root.canonicalize().map_err(|e|e.to_string())?;
 for part in rel.components(){path.push(part);if fs::symlink_metadata(&path).map_err(|_|"Only existing project files can be edited.")?.file_type().is_symlink(){return Err("Symlink files and directories cannot be edited.".into());}}
 let path=scoped(root,relative)?;
 if !path.is_file()||fs::metadata(&path).map_err(|e|e.to_string())?.len()>64_000||!project::files(root)?.contains(&path){return Err("Only included text project files below 64 KB can be edited.".into());}
 Ok(path)
}
pub fn validate(root:&Path,changes:&[FileEdit])->Result<()>{
 if changes.is_empty()||changes.len()>8{return Err("A project fix must contain between one and eight files.".into());}
 let mut paths=BTreeSet::new();
 for change in changes{
  let path=editable(root,&change.path)?;
  if !paths.insert(path.clone())||change.after.len()>64_000||change.after==change.before{return Err("Invalid or duplicate project file change.".into());}
  let actual=fs::read_to_string(path).map_err(|_|"Only UTF-8 project files can be edited.")?;
  if actual.contains('\0')||change.after.contains('\0'){return Err("Binary files cannot be edited by the assistant.".into());}
  if actual!=change.before{return Err("Project files changed since the proposed fix. Request a new diff before applying it.".into());}
  if redact(&actual,&[])!=actual||redact(&change.after,&[])!=change.after{return Err("Files containing credentials cannot be sent or changed by the assistant.".into());}
 }
 Ok(())
}
pub fn review(root:&Path,plan:&DeploymentPlan,advice:FixAdvice)->Result<FixReview>{
 if !advice.changes.is_empty(){validate(root,&advice.changes)?;}
 Ok(FixReview{id:id(),plan_id:plan.id.clone(),revision:plan.revision.clone(),explanation:advice.explanation,changes:advice.changes,expires_at:(chrono::Utc::now()+chrono::Duration::minutes(15)).to_rfc3339()})
}
// Originals are backed up before any replacement. No model-generated command is executed.
pub fn apply(root:&Path,review:&FixReview,backup_root:&Path)->Result<()>{
 use std::io::Write;use std::os::unix::fs::{OpenOptionsExt,PermissionsExt};
 if review.expires_at<now(){return Err("Project fix approval expired. Request a new diff.".into());}
 validate(root,&review.changes)?;
 let backup=backup_root.join(&review.id);fs::create_dir_all(&backup).map_err(|e|e.to_string())?;fs::set_permissions(&backup,fs::Permissions::from_mode(0o700)).map_err(|e|e.to_string())?;
 let paths:Vec<&str>=review.changes.iter().map(|c|c.path.as_str()).collect();
 let mut manifest=fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(backup.join("paths.json")).map_err(|e|e.to_string())?;manifest.write_all(serde_json::to_string_pretty(&paths).map_err(|e|e.to_string())?.as_bytes()).map_err(|e|e.to_string())?;
 let mut prepared=Vec::new();
 let prepare=(||->Result<()>{for (i,change) in review.changes.iter().enumerate(){
  let path=editable(root,&change.path)?;
  let mut saved=fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(backup.join(format!("{i}.original"))).map_err(|e|e.to_string())?;saved.write_all(change.before.as_bytes()).map_err(|e|e.to_string())?;saved.sync_all().map_err(|e|e.to_string())?;
  let temporary=path.with_file_name(format!(".liara-fix-{}-{i}",review.id));
  let mut file=fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&temporary).map_err(|e|e.to_string())?;
  prepared.push((path.clone(),temporary.clone()));
  file.write_all(change.after.as_bytes()).map_err(|e|e.to_string())?;file.set_permissions(fs::metadata(path).map_err(|e|e.to_string())?.permissions()).map_err(|e|e.to_string())?;file.sync_all().map_err(|e|e.to_string())?;
 }Ok(())})();
 if let Err(error)=prepare{for (_,temp) in &prepared{let _=fs::remove_file(temp);}return Err(error);}
 if let Err(error)=validate(root,&review.changes){for (_,temp) in &prepared{let _=fs::remove_file(temp);}return Err(error);}
 for (i,(path,temp)) in prepared.iter().enumerate(){
  if let Err(error)=fs::rename(temp,path){
   let mut rollback_failed=false;
   for (j,(previous,_)) in prepared.iter().take(i).enumerate(){if fs::write(previous,&review.changes[j].before).is_err(){rollback_failed=true;}}
   for (_,temp) in &prepared{let _=fs::remove_file(temp);}
   return Err(format!("Could not apply project fix: {error}. Originals are saved at {}. Rollback incomplete: {rollback_failed}",backup.display()));
  }
 }
 Ok(())
}
#[cfg(test)]mod tests{
 use super::*;
 fn change()->FileEdit{FileEdit{path:"app.py".into(),before:"print('old')\n".into(),after:"print('new')\n".into(),reason:"Correct startup behavior".into()}}
 fn request()->FixReview{FixReview{id:id(),plan_id:"plan".into(),revision:"revision".into(),explanation:String::new(),changes:vec![change()],expires_at:(chrono::Utc::now()+chrono::Duration::minutes(5)).to_rfc3339()}}
 #[test]fn api_and_quota_failures_are_not_application_code_repairs(){
  assert!(infrastructure_error("HTTP 400: not_enough_storage_space"));assert!(infrastructure_error("HTTP 500: upstream failure"));assert!(infrastructure_error("Network request failed (timeout)"));assert!(!infrastructure_error("shop-web release FAILED: invalid startup path"));
 }
 #[test]fn reviewed_changes_apply_with_backup_and_preserve_mode(){use std::os::unix::fs::PermissionsExt;let d=tempfile::tempdir().unwrap();let backup=tempfile::tempdir().unwrap();let c=change();fs::write(d.path().join(&c.path),&c.before).unwrap();fs::set_permissions(d.path().join(&c.path),fs::Permissions::from_mode(0o755)).unwrap();let r=request();apply(d.path(),&r,backup.path()).unwrap();assert_eq!(fs::read_to_string(d.path().join("app.py")).unwrap(),c.after);assert_eq!(fs::read_to_string(backup.path().join(&r.id).join("0.original")).unwrap(),c.before);assert_eq!(fs::metadata(d.path().join("app.py")).unwrap().permissions().mode()&0o777,0o755);}
 #[test]fn stale_expired_and_sensitive_diffs_never_modify_files(){let d=tempfile::tempdir().unwrap();let b=tempfile::tempdir().unwrap();fs::write(d.path().join("app.py"),"new user edit").unwrap();assert!(apply(d.path(),&request(),b.path()).is_err());assert_eq!(fs::read_to_string(d.path().join("app.py")).unwrap(),"new user edit");let mut r=request();fs::write(d.path().join("app.py"),&r.changes[0].before).unwrap();r.expires_at="2000-01-01T00:00:00Z".into();assert!(apply(d.path(),&r,b.path()).is_err());r=request();r.changes[0].path=".env".into();fs::write(d.path().join(".env"),&r.changes[0].before).unwrap();assert!(apply(d.path(),&r,b.path()).is_err());r=request();r.changes[0].after="password=private".into();assert!(apply(d.path(),&r,b.path()).is_err());}
 #[test]fn symlinks_traversal_and_partial_stale_batches_are_rejected(){let d=tempfile::tempdir().unwrap();let b=tempfile::tempdir().unwrap();fs::write(d.path().join("app.py"),change().before).unwrap();std::os::unix::fs::symlink("app.py",d.path().join("link.py")).unwrap();let mut c=change();c.path="link.py".into();assert!(validate(d.path(),&[c]).is_err());let mut c=change();c.path="../escape.py".into();assert!(validate(d.path(),&[c]).is_err());let mut r=request();let mut second=change();second.path="other.py".into();fs::write(d.path().join("other.py"),"user edit").unwrap();r.changes.push(second);assert!(apply(d.path(),&r,b.path()).is_err());assert_eq!(fs::read_to_string(d.path().join("app.py")).unwrap(),change().before);}
}
