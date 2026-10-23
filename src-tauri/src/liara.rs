use crate::{models::*,security::*};
use reqwest::{Client,Method};
use serde_json::{json,Value};
use std::time::Duration;

// Liara's deployment gateway uses the CLI product/version token for compatibility.
// Keep our own identity explicit; this is protocol compatibility, not a Node sidecar.
pub const DEPLOY_USER_AGENT:&str=concat!("@liara/cli/9.6.0 (compatible; liara-launch/",env!("CARGO_PKG_VERSION"),")");
pub const API:&str="https://api.liara.ir";
pub const STORAGE:&str="https://storage-service.liara.ir";
pub const DNS:&str="https://dns-service.liara.ir";
#[derive(Clone)]pub struct Liara {pub client:Client,token:String,pub team_id:Option<String>}
impl Liara {
    pub fn new(token:String,team_id:Option<String>)->Result<Self>{Ok(Self{client:Client::builder().user_agent(DEPLOY_USER_AGENT).timeout(Duration::from_secs(45)).connect_timeout(Duration::from_secs(15)).redirect(reqwest::redirect::Policy::none()).build().map_err(|e|e.to_string())?,token,team_id})}
    pub fn secret(&self)->String{self.token.clone()}
    pub fn scoped_account_id(&self,user_id:&str)->String{account_context_id(user_id,self.team_id.as_deref())}
    pub async fn request(&self,method:Method,base:&str,path:&str,body:Option<Value>)->Result<Value>{
        if ![API,STORAGE,DNS].contains(&base)||!path.starts_with('/') {return Err("Unrecognized Liara endpoint.".into());}
        let attempts=if method==Method::GET {3}else{1};
        for attempt in 0..attempts {
            let mut req=self.client.request(method.clone(),request_url(base,path,self.team_id.as_deref())?).bearer_auth(&self.token);
            if let Some(ref b)=body {req=req.json(b);}
            let response=match req.send().await {Ok(r)=>r,Err(e)=>{if attempt+1<attempts {tokio::time::sleep(Duration::from_secs(1<<attempt)).await;continue;}return Err(format!("Network request failed ({}). Check your connection; a submitted operation may still be running.",if e.is_timeout(){"timeout"}else{"connection"}));}};
            if (response.status().as_u16()==429||response.status().is_server_error())&&attempt+1<attempts {let wait=response.headers().get("retry-after").and_then(|h|h.to_str().ok()).and_then(|s|s.parse::<u64>().ok()).unwrap_or(2).min(30);tokio::time::sleep(Duration::from_secs(wait)).await;continue;}
            return parse_response(response,&self.token).await.map_err(|e|format!("{e} [{} {}]",method,path.split('?').next().unwrap_or(path)));
        }Err("Request failed".into())
    }
    pub async fn get(&self,path:&str)->Result<Value>{self.request(Method::GET,API,path,None).await}
    pub async fn post(&self,path:&str,body:Value)->Result<Value>{self.request(Method::POST,API,path,Some(body)).await}
    pub async fn upload(&self,name:&str,path:&std::path::Path)->Result<String>{
        let form=reqwest::multipart::Form::new().file("file",path).await.map_err(|_|"Could not read source archive.")?;
        let r=self.client.post(request_url(API,&format!("/v2/projects/{name}/sources"),self.team_id.as_deref())?).bearer_auth(&self.token).timeout(Duration::from_secs(900)).multipart(form).send().await.map_err(|_|"Upload interrupted. Check the connection before retrying.")?;
        let data=parse_response(r,&self.token).await?;data["sourceID"].as_str().map(str::to_string).ok_or("Liara upload response did not include sourceID.".into())
    }
    pub async fn account(&self,storage:&str)->Result<Account>{
        let me=self.get("/v1/me").await?;
        let user_id=me["user"]["_id"].as_str().ok_or("Liara returned an unexpected account response.")?;
        let id=self.scoped_account_id(user_id);
        let (billing,networks,projects,databases,buckets)=tokio::join!(self.get("/v1/billing"),self.get("/v1/networks"),self.get("/v1/projects"),self.get("/v1/databases"),self.request(Method::GET,STORAGE,"/api/v1/buckets",None));
        let mut warnings=vec![];
        fn list(result:Result<Value>,key:&str,warnings:&mut Vec<String>)->Vec<Value>{match result{Ok(v)=>v[key].as_array().map(|a|a.iter().map(sanitize_json).collect()).unwrap_or_default(),Err(e)=>{warnings.push(format!("{key}: {e}"));vec![]}}}
        let balance=account_balance(&me,billing,self.team_id.is_some(),&mut warnings);
        Ok(Account{id,name:me["user"]["fullname"].as_str().unwrap_or("Liara account").into(),email:me["user"]["email"].as_str().unwrap_or("").into(),balance,credential_storage:storage.into(),plans:normalize_plans(&me["plans"]),networks:list(networks,"networks",&mut warnings),projects:list(projects,"projects",&mut warnings),databases:list(databases,"databases",&mut warnings),buckets:list(buckets,"buckets",&mut warnings),database_versions:me["defaultDatabaseVersions"].clone(),warnings})
    }
}
// /v1/billing describes the personal user; /v1/me explicitly exposes team billing.
fn account_balance(me:&Value,billing:Result<Value>,team_mode:bool,warnings:&mut Vec<String>)->Option<f64>{
    if team_mode {
        let balance=me["team"]["balance"].as_f64();
        if balance.is_none(){warnings.push("Team balance is unavailable; personal account balance is not used.".into());}
        balance
    }else{match billing{Ok(v)=>v["user"]["balance"].as_f64(),Err(e)=>{warnings.push(format!("billing: {e}"));None}}}
}
pub(crate) fn account_context_id(user_id:&str,team_id:Option<&str>)->String{
    team_id.map(|team|hash(format!("{user_id}:team:{team}").as_bytes())).unwrap_or_else(||user_id.to_string())
}
fn request_url(base:&str,path:&str,team_id:Option<&str>)->Result<reqwest::Url>{
    let mut url=reqwest::Url::parse(&format!("{base}{path}")).map_err(|_|"Unrecognized Liara endpoint.")?;
    if let Some(team_id)=team_id{url.query_pairs_mut().append_pair("teamID",team_id);}
    Ok(url)
}
async fn parse_response(response:reqwest::Response,token:&str)->Result<Value>{
    let code=response.status().as_u16();
    let text=response.text().await.map_err(|_|"Could not read Liara response.")?;
    if !(200..300).contains(&code){
        let message=match code{401=>"Token was rejected. Create a new Liara API token.",403=>"This token does not have permission for this action.",402=>"Your Liara balance is insufficient. Add credit in the console.",409=>"A resource with this name already exists. Choose it explicitly or use another name.",429=>"Liara is rate limiting requests. Please retry shortly.",_=>"Liara could not complete this request."};
        return Err(format!("HTTP {code}: {message} {}",error_detail(&text,token)));
    }
    if text.trim().is_empty(){Ok(json!({}))}else{serde_json::from_str(&text).map_err(|_|"Liara returned a response with an unsupported format.".into())}
}
fn error_detail(text:&str,token:&str)->String{
    let parsed=serde_json::from_str::<Value>(text).ok();
    let detail=parsed.as_ref().and_then(|v|v["message"].as_str().or_else(||v["error"]["message"].as_str()).or_else(||v["error"].as_str()).or_else(||v.as_str()));
    // Do not dump arbitrary JSON (it may contain request environment variables) or HTML.
    let detail=detail.or_else(||if parsed.is_none()&&!text.trim_start().starts_with('<'){Some(text.trim())}else{None}).unwrap_or("");
    if detail.trim().is_empty(){return "Liara returned no error details.".into();}
    redact(detail,&[token.into()]).chars().take(500).collect()
}
pub fn normalize_plans(plans:&Value)->Vec<PlanOption>{
    let mut out=vec![];
    for (category,key,bundle_key) in [("app","projects","projectBundlePlans"),("database","databases","databaseBundlePlans"),("bucket","objectStorage","")] {
        let Some(items)=plans[key].as_object()else{continue};
        for (id,p) in items {
            if p["available"].as_bool()==Some(false){continue;}
            // Preserve unknown prices; an unknown price must never appear as free.
            let base=p["price"].as_f64().map(|n|n*720.0);
            let bundles=plans[bundle_key][id].as_object();
            let options:Vec<(String,Option<f64>)>=if let Some(b)=bundles {
                b.iter().filter(|(_,v)|v["available"].as_bool()!=Some(false)).map(|(k,v)|(k.clone(),v["displayPrice"].as_f64())).collect()
            }else{vec![("".into(),Some(0.0))]};
            for (bundle,price) in options {
                out.push(PlanOption{id:id.clone(),category:category.into(),label:p["name"].as_str().unwrap_or(id).into(),ram:p["RAM"]["amount"].as_f64().unwrap_or(0.0),cpu:p["CPU"]["amount"].as_f64().unwrap_or(0.0),disk:p["volume"].as_f64().unwrap_or(0.0),monthly_price:base.zip(price).map(|(a,b)|a+b),bundle_id:bundle,region:p["region"].as_str().unwrap_or("").into(),supports:p["supports"].as_array().map(|a|a.iter().filter_map(|x|x.as_str().map(str::to_string)).collect()).unwrap_or_default()});
            }
        }
    }
    out.sort_by(|a,b|a.monthly_price.unwrap_or(f64::INFINITY).total_cmp(&b.monthly_price.unwrap_or(f64::INFINITY)));out
}
#[cfg(test)]mod tests{use super::*;
 #[tokio::test]async fn versioned_client_identity_is_present_on_the_wire(){
  use tokio::io::{AsyncReadExt,AsyncWriteExt};
  let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();let address=listener.local_addr().unwrap();
  let server=tokio::spawn(async move{let(mut stream,_)=listener.accept().await.unwrap();let mut request=Vec::new();let mut buffer=[0u8;1024];while !request.windows(4).any(|s|s==b"\r\n\r\n"){let n=stream.read(&mut buffer).await.unwrap();assert!(n>0);request.extend_from_slice(&buffer[..n]);assert!(request.len()<16384);}let text=String::from_utf8(request).unwrap();assert!(text.to_lowercase().contains(&format!("user-agent: {}",DEPLOY_USER_AGENT.to_lowercase())));stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}").await.unwrap();});
  let api=Liara::new("test".into(),None).unwrap();assert!(api.client.get(format!("http://{address}/")).send().await.unwrap().status().is_success());server.await.unwrap();
 }
 #[test]fn server_error_details_preserve_safe_messages(){
  assert_eq!(error_detail("", "secret-token"),"Liara returned no error details.");
  assert_eq!(error_detail(r#"{"error":{"message":"Build service unavailable"}}"#,"secret-token"),"Build service unavailable");
  assert_eq!(error_detail("Upstream unavailable", "secret-token"),"Upstream unavailable");
  assert_eq!(error_detail(r#"{"envs":{"PASSWORD":"private"}}"#,"secret-token"),"Liara returned no error details.");
  assert!(!error_detail(r#"{"message":"Rejected secret-token"}"#,"secret-token").contains("secret-token"));
  assert_eq!(error_detail("<html>private proxy details</html>","secret-token"),"Liara returned no error details.");
 }
 #[test]fn team_id_is_added_to_every_api_url(){let url=request_url(API,"/v1/me",Some("team abc")).unwrap();assert_eq!(url.query(),Some("teamID=team+abc"));assert_eq!(request_url(API,"/v1/me",None).unwrap().query(),None);}
 #[test]fn scoped_account_id_matches_the_executor_context(){let api=Liara::new("token".into(),Some("team-7".into())).unwrap();assert_eq!(api.scoped_account_id("user-1"),account_context_id("user-1",Some("team-7")));assert_ne!(api.scoped_account_id("user-1"),"user-1");}
 #[test]fn team_contexts_have_separate_private_local_ids(){let team=account_context_id("user-1",Some("team-7"));assert_ne!(team,account_context_id("user-1",Some("team-8")));assert_ne!(team,account_context_id("user-1",None));assert!(!team.contains("team-7"));}
 #[test]fn team_balance_never_uses_personal_credit(){
    let mut warnings=vec![];
    let personal=||Ok(json!({"user":{"balance":8}}));
    assert_eq!(account_balance(&json!({"team":{"balance":20000000}}),personal(),true,&mut warnings),Some(20000000.0));
    assert_eq!(account_balance(&json!({"team":{"balance":0}}),personal(),true,&mut warnings),Some(0.0));
    assert_eq!(account_balance(&json!({}),personal(),true,&mut warnings),None);
    assert!(!warnings.is_empty());
    assert_eq!(account_balance(&json!({"team":{"balance":20000000}}),personal(),false,&mut warnings),Some(8.0));
 }
 #[test]fn accounts_for_bundle_cost(){let plans=normalize_plans(&json!({"projects":{"g2":{"available":true,"price":100,"RAM":{"amount":1}}},"projectBundlePlans":{"g2":{"free":{"available":true,"displayPrice":0},"pro":{"available":true,"displayPrice":20000}}}}));assert_eq!(plans[0].monthly_price,Some(72000.0));assert_eq!(plans[1].monthly_price,Some(92000.0));}
 #[test]fn unknown_is_not_free(){assert_eq!(normalize_plans(&json!({"projects":{"x":{"available":true}}}))[0].monthly_price,None);}
}

#[cfg(all(test,feature="desktop"))]mod live_diagnostics{
 use super::*;
 #[tokio::test]#[ignore="Read-only Liara diagnostic; requires a run ID and saved desktop credentials"]async fn inspect_saved_run_releases(){
  let run_id=std::env::var("LIARA_INSPECT_RUN_ID").expect("Set LIARA_INSPECT_RUN_ID");
  let home=std::env::var("HOME").unwrap();let data=std::env::var("XDG_DATA_HOME").unwrap_or(format!("{home}/.local/share"));let store=crate::store::Store::new(&std::path::Path::new(&data).join("ir.liara.launch.desktop")).unwrap();let run:DeploymentRun=store.get("run",&run_id).unwrap();
  let token=keyring::Entry::new("ir.liara.launch.desktop","liara-token").unwrap().get_password().expect("Saved app connection unavailable");let team=keyring::Entry::new("ir.liara.launch.desktop","liara-team-id").unwrap().get_password().ok();let api=Liara::new(token,team).unwrap();let me=api.get("/v1/me").await.unwrap();assert_eq!(api.scoped_account_id(me["user"]["_id"].as_str().unwrap()),run.plan.account_id,"Saved credentials do not match this run");
  for service in run.plan.services.iter().filter(|s|s.kind=="app"){
   let detail=api.get(&format!("/v1/projects/{}",service.name)).await.unwrap();
   let plan=detail["project"]["planID"].as_str().unwrap_or("");
   let disks=api.get(&format!("/v1/projects/{}/disks",detail["project"]["_id"].as_str().unwrap())).await.unwrap();
   println!("{}",json!({"app":service.name,"plan":plan,"capacityGb":me["plans"]["projects"][plan]["volume"],"disks":disks["disks"].as_array().map(|items|items.iter().map(|d|json!({"name":d["name"],"sizeGb":d["size"]})).collect::<Vec<_>>())}));
   let catalog=normalize_plans(&me["plans"]);
   let current=catalog.iter().find(|p|p.id==plan&&p.category=="app"&&p.bundle_id==service.bundle_id).unwrap();
   let required=crate::storage::required_storage(&service.volumes,disks["disks"].as_array().unwrap()).unwrap();
   let proposed=crate::storage::fitting_plan(&catalog,current,&service.platform,required).expect("No adequate compatible plan");
   println!("{}",json!({"requiredGb":required,"recommendedPlan":proposed.id,"recommendedCapacityGb":proposed.disk,"oldMonthlyTomans":current.monthly_price,"newMonthlyTomans":proposed.monthly_price}));
   if run.resources.iter().find(|r|r.service_id==service.id).unwrap().source_id.is_none(){let staging=store.stage_dir(&run.plan.id,&service.id);assert_eq!(crate::project::source_hash(&staging).unwrap(),service.source_hash);let copy=tempfile::tempdir().unwrap();let hash=crate::project::stage(&staging,&copy.path().join("recovery"),&[]).unwrap();assert_eq!(hash,service.source_hash,"Recovery snapshot changed");}


   let list=api.get(&format!("/v1/projects/{}/releases?page=1&count=100",service.name)).await.unwrap();
   println!("release count: {}",list["total"]);
   for item in list["releases"].as_array().unwrap().iter().take(5){
    let id=item["_id"].as_str().unwrap();let detail=api.get(&format!("/v1/releases/{id}")).await.unwrap();let release=&detail["release"];
    println!("{}",json!({"id":id,"state":item["state"],"createdAt":item["createdAt"],"keys":release.as_object().map(|o|o.keys().collect::<Vec<_>>())}));
   }
  }
 }
}
