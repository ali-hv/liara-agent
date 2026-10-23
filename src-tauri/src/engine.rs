use crate::{models::*,liara::{Liara,STORAGE},planner,project,security::*,store::Store};
use serde_json::{json,Value};
use crate::storage::required_storage;
use std::{collections::BTreeMap,sync::{Arc,atomic::{AtomicBool,Ordering}},time::Duration};

pub type Notify=Arc<dyn Fn(&DeploymentRun)+Send+Sync>;
pub struct Executor {pub api:Liara,pub store:Arc<Store>,pub notify:Notify,pub stop:Arc<AtomicBool>,pub secrets:Secrets,pub redactions:Vec<String>}
impl Executor {
 fn save(&self,run:&mut DeploymentRun)->Result<()>{run.updated_at=now();self.store.save("run",&run.id,&run.plan.account_id,run)?;(self.notify)(run);Ok(())}
 fn event(&self,run:&mut DeploymentRun,service:Option<&str>,level:&str,message:&str)->Result<()>{
    run.events.push(RunEvent{at:now(),level:level.into(),service_id:service.map(str::to_string),message:redact(message,&self.redactions)});
    if run.events.len()>1500{run.events.drain(0..200);}
    self.save(run)
 }
 fn check_stop(&self)->Result<()>{if self.stop.load(Ordering::Relaxed){Err("Stopped locally. Already submitted Liara builds may continue; resources have been preserved.".into())}else{Ok(())}}
 pub async fn execute(mut self,mut run:DeploymentRun){
    let result=self.run(&mut run).await;
    if let Err(e)=result {run.status=if self.stop.load(Ordering::Relaxed){"paused"}else{"failed"}.into();let _=self.event(&mut run,None,"error",&e);let _=self.event(&mut run,None,"warning","Existing resources were preserved and may continue to incur charges. Resume reconciles saved IDs; nothing is deleted automatically.");}
 }
 async fn run(&mut self,run:&mut DeploymentRun)->Result<()>{
    run.status="running".into();self.event(run,None,"info","Checking account, approved plan and saved deployment state…")?;
    let me=self.api.get("/v1/me").await?;
    let user_id=me["user"]["_id"].as_str().ok_or("Liara returned an unexpected account response.")?;
    // Team-scoped local account IDs are private hashes, while /v1/me returns
    // the underlying user ID. Recreate the same scope before checking ownership.
    let active_account_id=self.api.scoped_account_id(user_id);
    if active_account_id!=run.plan.account_id{return Err("This deployment belongs to a different account.".into());}
    // Revalidate advertised prices before any provisioning; never silently accept an increase.
    let current=crate::liara::normalize_plans(&me["plans"]);
    for s in &run.plan.services {if s.existing_id.is_none()&&run.resources.iter().find(|r|r.service_id==s.id).is_none_or(|r|r.resource_id.is_none()) {
        let p=current.iter().find(|p|p.category==s.kind&&p.id==s.plan_id&&p.bundle_id==s.bundle_id).ok_or("An approved plan is no longer available. Prepare a new plan.")?;
        if p.monthly_price!=s.monthly_price{return Err("Pricing changed since approval. Prepare and approve a refreshed plan.".into());}
    }}
    for service in &run.plan.services{if let Some(resize)=&service.resize{
        let target=current.iter().find(|p|p.category=="app"&&p.id==service.plan_id&&p.bundle_id==service.bundle_id).ok_or("The recommended upgrade is no longer available. Review a new recommendation.")?;
        if target.monthly_price!=Some(resize.target_monthly_price)||target.disk<resize.required_gb{return Err("Upgrade pricing or storage changed. Review the recommendation again.".into());}
    }}
    if run.network_id.is_none(){if let Some(network)=run.plan.network.clone(){
        if let Some(name)=network.strip_prefix("new:"){
            let networks=self.api.get("/v1/networks").await?;
            let found=networks["networks"].as_array().and_then(|a|a.iter().find(|n|n["name"].as_str()==Some(name))).and_then(|n|n["_id"].as_str()).map(str::to_string);
            if let Some(found)=found {if !run.network_pending{return Err("A network with the approved new name now exists. Refresh the plan and select it explicitly.".into());}run.network_id=Some(found);run.network_pending=false;self.save(run)?;}
            else if run.network_pending {return Err("Network creation has an uncertain result. Recheck the Liara console before preparing a new plan.".into());}
            else {run.network_pending=true;self.save(run)?;self.api.post("/v1/networks",json!({"networkName":name})).await?;let networks=self.api.get("/v1/networks").await?;run.network_id=networks["networks"].as_array().and_then(|a|a.iter().find(|n|n["name"].as_str()==Some(name))).and_then(|n|n["_id"].as_str()).map(str::to_string);if run.network_id.is_none(){return Err("Network created but its ID is not yet available. Resume to reconcile.".into());}run.network_pending=false;self.save(run)?;}
        }else{run.network_id=Some(network);self.save(run)?;}
    }}
    let order=planner::order(&run.plan.services)?;
    let mut connections:BTreeMap<String,BTreeMap<String,String>>=BTreeMap::new();
    for index in order {
        self.check_stop()?;
        let s=run.plan.services[index].clone();
        let ri=run.resources.iter().position(|r|r.service_id==s.id).ok_or("Missing progress record")?;
        self.event(run,Some(&s.id),"info",&format!("Preparing {}",s.name))?;
        if run.resources[ri].resource_id.is_none(){
            if let Some(id)=s.existing_id.clone(){run.resources[ri].resource_id=Some(id);self.save(run)?;}
            else {
                let found=self.find_resource(&s).await?;
                if let Some(id)=found {
                    if run.resources[ri].request_pending.as_deref()!=Some("create"){return Err(format!("{} now exists. Select it explicitly in a new plan.",s.name));}
                    run.resources[ri].resource_id=Some(id);run.resources[ri].request_pending=None;self.save(run)?;
                }else {
                    if run.resources[ri].request_pending.is_some(){return Err(format!("{}: previous resource request has an uncertain outcome. No duplicate request was sent.",s.name));}
                    run.resources[ri].request_pending=Some("create".into());self.save(run)?;
                    match s.kind.as_str(){
                        "app"=>{self.api.post("/v1/projects",json!({"name":s.name,"platform":s.platform,"planID":s.plan_id,"bundlePlanID":s.bundle_id,"network":run.network_id})).await?;},
                        "database"=>{self.api.post("/v1/databases",json!({"hostname":s.name,"type":s.platform,"version":s.version,"planID":s.plan_id,"bundlePlanID":s.bundle_id,"publicNetwork":false,"network":run.network_id})).await?;},
                        "bucket"=>{self.api.request(reqwest::Method::POST,STORAGE,"/api/v1/buckets",Some(json!({"name":s.name,"plan":s.plan_id,"permission":"private"}))).await?;},
                        _=>return Err("Unsupported resource kind".into())
                    }
                    for _ in 0..15{if let Some(id)=self.find_resource(&s).await? {run.resources[ri].resource_id=Some(id);break;}self.check_stop()?;tokio::time::sleep(Duration::from_secs(2)).await;}
                    if run.resources[ri].resource_id.is_none(){return Err("Resource request succeeded but its ID is not yet visible. Resume to reconcile.".into());}
                    run.resources[ri].request_pending=None;self.save(run)?;
                }
            }
        }
        if s.kind=="database" {
            let id=run.resources[ri].resource_id.clone().unwrap();
            let mut details=None;
            for _ in 0..90 {self.check_stop()?;let value=self.api.get(&format!("/v1/databases/{id}")).await?;let d=&value["database"];
                if d["type"].as_str()!=Some(&s.platform){return Err("Existing database type does not match the approved plan.".into());}
                if !d["root_password"].as_str().unwrap_or("").is_empty()&&d["internalPort"].as_u64().is_some(){details=Some(d.clone());break;}tokio::time::sleep(Duration::from_secs(3)).await;
            }
            let d=details.ok_or("Database is still initializing. Resume later to continue.")?;
            let values=database_connection(&d,&s.platform)?;self.redactions.extend(values.values().filter(|v|v.len()>3).cloned());connections.insert(s.id.clone(),values);
            run.resources[ri].phase="ready".into();self.event(run,Some(&s.id),"success","Managed database is ready. Connection values remain inside the native backend.")?;continue;
        }
        if s.kind=="bucket" {connections.insert(s.id.clone(),BTreeMap::from([("bucket".into(),s.name.clone())]));run.resources[ri].phase="ready".into();self.save(run)?;continue;}
        let resource_id=run.resources[ri].resource_id.clone().unwrap();
        let app_name=&s.name;
        let mut detail=self.api.get(&format!("/v1/projects/{app_name}")).await?;
        let name=detail["project"]["project_id"].as_str().unwrap_or(&s.name).to_string();
        if detail["project"]["type"].as_str()!=Some(&s.platform){return Err("Existing application platform does not match the approved plan.".into());}
        if run.resources[ri].request_pending.as_deref().is_some_and(|p|p.starts_with("disk:"))&&run.events.iter().rev().find(|e|e.level=="error").is_some_and(|e|storage_rejection(&e.message,&name)){
            run.resources[ri].request_pending=None;self.save(run)?;
        }
        if let Some(resize)=&s.resize{
            let pending=format!("resize:{}",s.plan_id);
            let actual=detail["project"]["planID"].as_str().ok_or("Current application plan unavailable.")?;
            let step=crate::storage::resize_step(actual,&resize.from_plan_id,&s.plan_id,run.resources[ri].request_pending.as_deref())?;
            if step==crate::storage::ResizeStep::Uncertain{return Err(if run.plan.language=="fa"{"نتیجه ارتقای پلن هنوز روشن نیست. پس از چند لحظه دوباره ادامه دهید؛ درخواست ارتقای تکراری ارسال نشد."}else{"The previous upgrade has not been confirmed yet. Resume later to reconcile it; no duplicate resize request was sent."}.into());}
            if step==crate::storage::ResizeStep::Submit{
                run.resources[ri].request_pending=Some(pending.clone());self.save(run)?;
                self.event(run,Some(&s.id),"info",&crate::storage::explain(&s,&run.plan.language))?;
                if let Err(error)=self.api.post(&format!("/v1/projects/{name}/resize"),json!({"planID":s.plan_id})).await{
                    if ["HTTP 400:","HTTP 401:","HTTP 402:","HTTP 403:"].iter().any(|code|error.starts_with(code)){run.resources[ri].request_pending=None;self.save(run)?;}
                    return Err(error);
                }
                for _ in 0..15{self.check_stop()?;detail=self.api.get(&format!("/v1/projects/{name}")).await?;if detail["project"]["planID"].as_str()==Some(&s.plan_id){break;}tokio::time::sleep(Duration::from_secs(2)).await;}
                if detail["project"]["planID"].as_str()!=Some(&s.plan_id){return Err("The plan upgrade was requested but is not confirmed yet. Resume later; the same request will not be resubmitted.".into());}
            }
            if run.resources[ri].request_pending.as_deref()==Some(&pending){run.resources[ri].request_pending=None;self.save(run)?;}
        }
        if s.volumes.iter().any(|v|v.resolution.as_deref()==Some("disk")){
            let disks=self.api.get(&format!("/v1/projects/{resource_id}/disks")).await?;
            let items=disks["disks"].as_array().ok_or("Liara returned an unrecognized disk list. No disk was created.")?;
            if let Some(capacity)=detail["project"]["planID"].as_str().and_then(|id|current.iter().find(|p|p.category=="app"&&p.id==id)).map(|p|p.disk){
                let required=required_storage(&s.volumes,items)?;
                if required>capacity{return Err(planner::storage_capacity_message(&name,required,capacity,&run.plan.language));}
            }
        }
        for volume in s.volumes.iter().filter(|v|v.resolution.as_deref()==Some("disk")) {
            let disk_name=volume.disk_name.as_deref().ok_or("Approved disk name is missing")?;
            let size=volume.size_gb.ok_or("Approved disk size is missing")?;
            let pending=format!("disk:{disk_name}");

            let disks=self.api.get(&format!("/v1/projects/{resource_id}/disks")).await?;
            let listed=disks["disks"].as_array().and_then(|items|items.iter().find(|d|d["name"].as_str()==Some(disk_name)));
            if let Some(disk)=listed {
                let actual=disk["size"].as_u64();
                if actual.is_some_and(|actual|actual!=size as u64){return Err(format!("Disk {disk_name} already exists at a different size. Review it in Liara before changing the plan."));}
                if run.resources[ri].request_pending.as_deref()==Some(pending.as_str()){run.resources[ri].request_pending=None;self.save(run)?;}
            }else {
                if run.resources[ri].request_pending.as_deref()==Some(pending.as_str()){return Err(format!("Disk {disk_name} creation has an uncertain result. Check the Liara console before resuming; no duplicate was created."));}
                if run.resources[ri].request_pending.is_some(){return Err(format!("{} has an unfinished Liara request. Reconcile it in the console before continuing.",s.name));}
                run.resources[ri].request_pending=Some(pending.clone());self.save(run)?;
                if let Err(error)=self.api.post(&format!("/v1/projects/{name}/disks"),json!({"name":disk_name,"size":size})).await{
                    if storage_rejection(&error,&name){run.resources[ri].request_pending=None;self.save(run)?;}
                    return Err(error);
                }
                let mut created=false;
                for _ in 0..15 {let current=self.api.get(&format!("/v1/projects/{resource_id}/disks")).await?;if current["disks"].as_array().is_some_and(|items|items.iter().any(|d|d["name"].as_str()==Some(disk_name))){created=true;break;}self.check_stop()?;tokio::time::sleep(Duration::from_secs(2)).await;}
                if !created{return Err(format!("Disk {disk_name} was requested but is not visible yet. Resume later to reconcile the same request."));}
                run.resources[ri].request_pending=None;self.save(run)?;
            }
        }
        connections.insert(s.id.clone(),BTreeMap::from([("url".into(),format!("https://{name}.liara.run")),("host".into(),name.clone()),("port".into(),s.port.to_string())]));
        if run.resources[ri].phase=="verified"{continue;}
        let mut envs=self.secrets.get(&s.id).cloned().unwrap_or_default();
        apply_bindings(&mut envs,&s.bindings,&connections)?;
        self.redactions.extend(envs.values().filter(|v|!v.is_empty()).cloned());
        if run.resources[ri].release_id.is_none(){
            if run.resources[ri].request_pending.as_deref()==Some("release")&&run.events.iter().rev().find(|e|e.level=="error").is_some_and(|e|rejected_release(&e.message,&name)){
                run.resources[ri].request_pending=None;self.save(run)?;
            }
            if run.resources[ri].request_pending.as_deref()==Some("release"){return Err("The last release submission has an uncertain outcome. Inspect release history in Liara before approving a new deployment; automatic resubmission is disabled.".into());}
            if s.image.is_none()&&run.resources[ri].source_id.is_none(){
                let dir=self.store.stage_dir(&run.plan.id,&s.id);
                if project::source_hash(&dir)?!=s.source_hash{return Err("Staged files changed after approval. Prepare a new deployment plan.".into());}
                self.event(run,Some(&s.id),"info","Packaging and uploading the approved source snapshot…")?;
                let archive=dir.with_extension("tar.gz");project::archive(&dir,&archive)?;
                let upload=self.api.upload(&name,&archive).await;let _=std::fs::remove_file(archive);
                run.resources[ri].source_id=Some(upload?);self.save(run)?;
            }
            // Environment updates are merged to preserve unmentioned existing values.
            if !envs.is_empty(){
                let mut merged:BTreeMap<String,String>=detail["project"]["envs"].as_array().map(|a|a.iter().filter_map(|e|Some((e["key"].as_str()?.into(),e["value"].as_str()?.into()))).collect()).unwrap_or_default();
                merged.extend(envs.clone());
                self.api.post("/v1/projects/update-envs",json!({"project":name,"variables":merged.iter().map(|(key,value)|json!({"key":key,"value":value})).collect::<Vec<_>>()})).await?;
            }
            let body=release_payload(&s,&run.id,&envs,run.resources[ri].source_id.as_deref())?;
            run.resources[ri].request_pending=Some("release".into());self.save(run)?;
            let response=match self.api.post(&format!("/v2/projects/{name}/releases"),body).await{
                Ok(response)=>response,
                Err(error)=>{if rejected_release(&error,&name){run.resources[ri].request_pending=None;self.save(run)?;}return Err(error);}
            };
            run.resources[ri].release_id=response["releaseID"].as_str().map(str::to_string);
            if run.resources[ri].release_id.is_none(){return Err("Release submission response omitted its ID. No automatic resubmission will occur.".into());}
            run.resources[ri].request_pending=None;run.resources[ri].phase="building".into();self.save(run)?;
        }
        let release=run.resources[ri].release_id.clone().unwrap();
        let mut since=String::new();let mut last_state=String::new();let mut ready=false;
        for _ in 0..400 {
            self.check_stop()?;
            let details=self.api.get(&format!("/v1/releases/{release}")).await?;
            let state=details["release"]["state"].as_str().unwrap_or("UNKNOWN").to_string();
            if state!=last_state{run.resources[ri].phase=state.to_lowercase();self.event(run,Some(&s.id),"info",&format!("Release: {state}"))?;last_state=state.clone();}
            if let Ok(logs)=self.api.get(&format!("/v2/releases/{release}/build-logs?since={}",percent_encoding::utf8_percent_encode(&since,percent_encoding::NON_ALPHANUMERIC))).await {
                if let Some(lines)=logs["buildOutput"].as_array(){for line in lines.iter().take(100){if let Some(text)=line["line"].as_str(){self.event(run,Some(&s.id),"log",text)?;}if let Some(t)=line["createdAt"].as_str(){since=t.into();}}}
            }
            if state=="READY" {ready=true;break;}
            if ["FAILED","UNHEALTHY","CANCELED","TIMEDOUT"].contains(&state.as_str()){
                if let Ok(logs)=self.api.get(&format!("/v1/projects/{name}/logs")).await{self.event(run,Some(&s.id),"log",&sanitize_json(&logs).to_string().chars().take(12000).collect::<String>())?;}
                return Err(format!("{} release {}: {}. Ask the assistant to inspect the failure and prepare a reviewed fix.",s.name,state,details["release"]["failReason"].as_str().unwrap_or("See deployment logs")));
            }
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
        if !ready{return Err("Build is still running. Resume later to check the same release.".into());}
        let url=format!("https://{name}.liara.run");
        let http=reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).timeout(Duration::from_secs(15)).build().map_err(|e|e.to_string())?;
        let mut verified=false;
        for _ in 0..10 {self.check_stop()?;if let Ok(r)=http.get(&url).send().await{if r.status().is_success()||r.status().is_redirection(){verified=true;break;}}tokio::time::sleep(Duration::from_secs(3)).await;}
        run.resources[ri].url=Some(url);
        if !verified{run.resources[ri].phase="unverified".into();self.save(run)?;return Err("Liara reports READY, but the public URL did not return a successful HTTP response. Check the app route or logs; success has not been assumed.".into());}
        run.resources[ri].phase="verified".into();self.event(run,Some(&s.id),"success","Release is ready and its public URL responded successfully.")?;
    }
    run.status="succeeded".into();self.event(run,None,"success","Deployment verified. Your project is live.")?;Ok(())
 }
 async fn find_resource(&self,s:&ServicePlan)->Result<Option<String>>{
    let (v,key,name_key,ids)=match s.kind.as_str(){"database"=>(self.api.get("/v1/databases").await?,"databases","hostname",vec!["DBId","id","_id"]),"bucket"=>(self.api.request(reqwest::Method::GET,STORAGE,"/api/v1/buckets",None).await?,"buckets","name",vec!["id"]),_=>(self.api.get("/v1/projects").await?,"projects","project_id",vec!["_id","project_id"])};
    Ok(v[key].as_array().and_then(|a|a.iter().find(|r|r[name_key].as_str()==Some(&s.name))).and_then(|r|ids.iter().find_map(|key|r[key].as_str())).map(str::to_string))
 }
}
// Keep this contract aligned with the official CLI deploy.ts, including build options.
fn release_payload(service:&ServicePlan,run_id:&str,envs:&BTreeMap<String,String>,source:Option<&str>)->Result<Value>{
 let mut body=json!({"build":{"cache":true},"port":service.port,"type":service.platform,"message":format!("Liara Launch {run_id}"),"envs":envs,"disks":release_mounts(&service.volumes)});
 if let Some(image)=&service.image{body["image"]=json!(image);}else{
  body["sourceID"]=json!(source.ok_or("No uploaded source is available for this release.")?);
  body["platformConfig"]=json!({});
  // The approved staging copy deliberately excludes .git; do not invent commit metadata.
  body["gitInfo"]=json!({"branch":null,"message":null,"commit":null,"committedAt":null,"remote":null,"author":{"email":null,"name":null},"tags":[]});
 }
 Ok(body)
}
// A 500 is never automatically retried. These checks only prepare an explicit retry approval.
pub(crate) fn release_retry_candidates(run:&DeploymentRun)->Vec<String>{
 run.resources.iter().filter(|r|r.request_pending.as_deref()==Some("release")&&r.release_id.is_none()&&r.resource_id.is_some()&&
  run.plan.services.iter().find(|s|s.id==r.service_id).is_some_and(|s|s.kind=="app"&&(s.image.is_some()||r.source_id.is_some())&&
   run.events.iter().rev().find(|e|e.level=="error"&&e.message.ends_with(&format!("[POST /v2/projects/{}/releases]",s.name))).is_some_and(|e|
    e.message.starts_with("HTTP 500:")&&chrono::DateTime::parse_from_rfc3339(&e.at).is_ok_and(|at|chrono::Utc::now().signed_duration_since(at).num_seconds()>=60))))
 .map(|r|r.service_id.clone()).collect()
}
pub(crate) fn empty_release_history(history:&Value)->bool{
 history["total"].as_u64()==Some(0)&&history["releases"].as_array().is_some_and(|items|items.is_empty())
}
pub(crate) async fn verify_release_retry(api:&Liara,run:&DeploymentRun,service_id:&str)->Result<()>{
 let fa=run.plan.language=="fa";
 if !release_retry_candidates(run).iter().any(|id|id==service_id){return Err(if fa{"وضعیت تلاش قبلی تغییر کرده است. دوباره ادامهٔ استقرار را بررسی کنید."}else{"Previous submission state changed. Review deployment recovery again."}.into());}
 let service=run.plan.services.iter().find(|s|s.id==service_id).ok_or("Unknown service")?;
 let history=api.get(&format!("/v1/projects/{}/releases?page=1&count=100",service.name)).await?;
 if !empty_release_history(&history){return Err(if fa{format!("برای {} نمی‌توان نبود نسخهٔ قبلی را تأیید کرد. پیش از تلاش دوباره، تاریخچهٔ استقرار را در کنسول لیارا بررسی کنید؛ درخواست تکراری ارسال نشد.",service.name)}else{format!("Cannot confirm an empty release history for {}. Inspect the Liara console before retrying; no duplicate submission was sent.",service.name)});}
 Ok(())
}
fn storage_rejection(error:&str,name:&str)->bool{
 error.starts_with("HTTP 400:")&&error.contains("not_enough_storage_space")&&error.ends_with(&format!("[POST /v1/projects/{name}/disks]"))
}
fn release_mounts(volumes:&[VolumeMount])->Vec<Value>{volumes.iter().filter(|v|v.resolution.as_deref()==Some("disk")).map(|v|json!({"name":v.disk_name,"mountTo":v.target})).collect()}
fn apply_bindings(envs:&mut BTreeMap<String,String>,bindings:&BTreeMap<String,String>,connections:&BTreeMap<String,BTreeMap<String,String>>)->Result<()>{
 for (key,binding) in bindings{let (dep,field)=binding.split_once('.').ok_or("Invalid binding")?;
  let connection=connections.get(dep);
  if let Some(existing)=envs.get(key){
   // Repair only our exact legacy default URL; preserve unrelated explicit overrides.
   let legacy_default=field=="url"&&connection.is_some_and(|c|c.get("database").map(String::as_str)==Some("postgres")&&c.get("url").and_then(|url|url.strip_suffix("postgres"))==Some(existing.as_str()));
   if !legacy_default{continue;}
  }
  let value=connection.and_then(|m|m.get(field)).ok_or_else(||format!("Cannot resolve {binding}; provide verified connection information in a revised plan."))?;envs.insert(key.clone(),value.clone());
 }Ok(())
}
// A validation rejection is a definite failure, unlike a timeout with an unknown outcome.
fn rejected_release(error:&str,name:&str)->bool{
 error.starts_with("HTTP 400:")&&error.ends_with(&format!("[POST /v2/projects/{name}/releases]"))
}
fn database_connection(d:&Value,kind:&str)->Result<BTreeMap<String,String>>{
 let host=d["hostname"].as_str().ok_or("Database hostname is unavailable")?;
 let port=d["internalPort"].as_u64().ok_or("Private database port is unavailable")?.to_string();
 let user=d["username"].as_str().unwrap_or("");let pass=d["root_password"].as_str().ok_or("Database credentials are unavailable")?;let db=d["dbName"].as_str().filter(|name|!name.is_empty()).unwrap_or(if kind=="postgres"{"postgres"}else{""});
 let encode=|s:&str|percent_encoding::utf8_percent_encode(s,percent_encoding::NON_ALPHANUMERIC).to_string();
 let scheme=match kind{"postgres"=>"postgresql","mariadb"=>"mysql","rabbitmq"=>"amqp",x=>x};
 let url=if kind=="redis"{format!("redis://:{}@{host}:{port}",encode(pass))}else{format!("{scheme}://{}:{}@{host}:{port}/{}{}",encode(user),encode(pass),encode(db),if kind=="mongodb"{"?authSource=admin"}else{""})};
 Ok(BTreeMap::from([("url".into(),url),("host".into(),host.into()),("port".into(),port),("username".into(),user.into()),("password".into(),pass.into()),("database".into(),db.into())]))
}
#[cfg(test)]mod tests {use super::*;
 fn release_fixture()->DeploymentRun{
  let service=ServicePlan{resize:None,id:"web".into(),name:"shop-web".into(),kind:"app".into(),platform:"docker".into(),root:".".into(),port:8080,image:None,plan_id:"small".into(),bundle_id:"basic".into(),monthly_price:None,existing_id:None,version:None,local_env_keys:vec![],env_keys:vec![],bindings:BTreeMap::new(),dependencies:vec![],volumes:vec![],files:vec![],source_hash:"hash".into(),rationale:String::new(),command:None};
  DeploymentRun{id:"run".into(),plan:DeploymentPlan{id:"plan".into(),project_id:"project".into(),account_id:"account".into(),revision:"revision".into(),network:None,services:vec![service],summary:String::new(),warnings:vec![],blockers:vec![],created_at:now(),language:"fa".into(),allow_config_repairs:false},status:"failed".into(),started_at:now(),updated_at:now(),network_id:None,network_pending:false,resources:vec![ResourceProgress{service_id:"web".into(),resource_id:Some("app-id".into()),source_id:Some("source.tar.gz".into()),request_pending:Some("release".into()),..Default::default()}],events:vec![RunEvent{at:(chrono::Utc::now()-chrono::Duration::minutes(5)).to_rfc3339(),level:"error".into(),service_id:None,message:"HTTP 500: failed [POST /v2/projects/shop-web/releases]".into()}]}
 }
 #[test]fn source_and_image_release_contracts_match_cli(){
  let mut service=release_fixture().plan.services.remove(0);let envs=BTreeMap::from([("DB_HOST".into(),"db".into())]);
  let source=release_payload(&service,"run",&envs,Some("source.tar.gz")).unwrap();
  assert_eq!(source["build"],json!({"cache":true}));assert_eq!(source["platformConfig"],json!({}));assert_eq!(source["gitInfo"]["tags"],json!([]));assert_eq!(source["envs"],json!({"DB_HOST":"db"}));assert_eq!(source["sourceID"],"source.tar.gz");assert!(source.get("image").is_none());assert!(release_payload(&service,"run",&envs,None).is_err());
  service.image=Some("nginx:alpine".into());let image=release_payload(&service,"run",&envs,None).unwrap();assert_eq!(image["image"],"nginx:alpine");assert!(image.get("sourceID").is_none());assert!(image.get("gitInfo").is_none());assert!(image.get("platformConfig").is_none());
 }
 #[test]fn retry_reconciliation_requires_explicit_empty_history(){
  assert!(empty_release_history(&json!({"total":0,"releases":[]})));
  for invalid in [json!({}),json!({"releases":[]}),json!({"total":0}),json!({"total":1,"releases":[]}),json!({"total":0,"releases":[{"_id":"existing"}]}),json!({"total":"0","releases":[]})]{assert!(!empty_release_history(&invalid));}
 }
 #[test]fn retry_is_limited_to_old_server_errors_without_release_ids(){
  let run=release_fixture();assert_eq!(release_retry_candidates(&run),vec!["web"]);
  let mut changed=run.clone();changed.events[0].at=now();assert!(release_retry_candidates(&changed).is_empty());
  for message in ["Network request failed (timeout)","HTTP 400: invalid [POST /v2/projects/shop-web/releases]","HTTP 500: failure [POST /v2/projects/other/releases]"]{let mut changed=run.clone();changed.events[0].message=message.into();assert!(release_retry_candidates(&changed).is_empty());}
  let mut changed=run.clone();changed.resources[0].release_id=Some("release-id".into());assert!(release_retry_candidates(&changed).is_empty());
  let mut changed=run.clone();changed.resources[0].source_id=None;assert!(release_retry_candidates(&changed).is_empty());
  let mut changed=run;changed.resources[0].request_pending=None;assert!(release_retry_candidates(&changed).is_empty());
 }
 #[test]fn capacity_counts_existing_and_new_disks_without_double_counting(){
  let volumes=vec![VolumeMount{resolution:Some("disk".into()),disk_name:Some("media".into()),size_gb:Some(5),..Default::default()},VolumeMount{resolution:Some("disk".into()),disk_name:Some("static".into()),size_gb:Some(2),..Default::default()}];
  assert_eq!(required_storage(&volumes,&[json!({"name":"media","size":5})]).unwrap(),7.0);
  assert_eq!(required_storage(&volumes,&[json!({"name":"media","size":5}),json!({"name":"old-data","size":3})]).unwrap(),10.0);
  assert!(required_storage(&volumes,&[json!({"name":"media"})]).is_err());
 }
 #[test]fn only_definitive_storage_rejections_clear_pending_disk_requests(){
  assert!(storage_rejection("HTTP 400: not_enough_storage_space [POST /v1/projects/shop-web/disks]","shop-web"));
  assert!(!storage_rejection("HTTP 500: not_enough_storage_space [POST /v1/projects/shop-web/disks]","shop-web"));
  assert!(!storage_rejection("HTTP 400: not_enough_storage_space [POST /v1/projects/other/disks]","shop-web"));
  assert!(!storage_rejection("Network request failed (timeout)","shop-web"));
 }
 #[test]fn release_disk_contract_contains_persistent_mounts(){
 let mounts=release_mounts(&[VolumeMount{target:"/app/media".into(),disk_name:Some("media".into()),resolution:Some("disk".into()),..Default::default()},VolumeMount{target:"/etc/nginx/config".into(),resolution:Some("copy".into()),..Default::default()}]);assert_eq!(mounts,vec![json!({"name":"media","mountTo":"/app/media"})]);
 }
 #[test]fn editable_values_override_automatic_connection_defaults(){
  let mut envs=BTreeMap::from([("DB_HOST".into(),"my-existing-db".into())]);let bindings=BTreeMap::from([("DB_HOST".into(),"db.host".into()),("DB_PASSWORD".into(),"db.password".into())]);let connections=BTreeMap::from([("db".into(),BTreeMap::from([("host".into(),"managed-db".into()),("password".into(),"generated-password".into())]))]);
  apply_bindings(&mut envs,&bindings,&connections).unwrap();assert_eq!(envs["DB_HOST"],"my-existing-db");assert_eq!(envs["DB_PASSWORD"],"generated-password");
 }
 #[test]fn only_explicit_release_validation_failures_allow_retry(){
 assert!(rejected_release("HTTP 400: envs must be an object [POST /v2/projects/shop-web/releases]","shop-web"));
 assert!(!rejected_release("HTTP 500: failure [POST /v2/projects/shop-web/releases]","shop-web"));
 assert!(!rejected_release("HTTP 400: failure [POST /v2/projects/other/releases]","shop-web"));
 assert!(!rejected_release("Network request failed (timeout)","shop-web"));
 }
 #[test]fn managed_postgres_defaults_to_the_documented_database_and_repairs_only_legacy_urls(){
  let values=database_connection(&json!({"hostname":"db","internalPort":5432,"username":"root","root_password":"secret"}),"postgres").unwrap();assert_eq!(values["database"],"postgres");assert!(values["url"].ends_with("/postgres"));
  let connections=BTreeMap::from([("db".into(),values.clone())]);let bindings=BTreeMap::from([("DATABASE_URL".into(),"db.url".into())]);let mut envs=BTreeMap::from([("DATABASE_URL".into(),values["url"].strip_suffix("postgres").unwrap().to_string())]);apply_bindings(&mut envs,&bindings,&connections).unwrap();assert_eq!(envs["DATABASE_URL"],values["url"]);
  envs.insert("DATABASE_URL".into(),"postgresql://custom-user:custom-secret@db:5432/".into());apply_bindings(&mut envs,&bindings,&connections).unwrap();assert_eq!(envs["DATABASE_URL"],"postgresql://custom-user:custom-secret@db:5432/");
 }
 #[test]fn connection_values_escape_passwords(){let c=database_connection(&json!({"hostname":"db","internalPort":5432,"username":"root","root_password":"a@b:c","dbName":"app"}),"postgres").unwrap();assert!(c["url"].contains("a%40b%3Ac"));assert_eq!(c["host"],"db");}
 #[test]fn rabbitmq_connection_uses_amqp(){let c=database_connection(&json!({"hostname":"broker","internalPort":5672,"username":"user","root_password":"pass","dbName":"/"}),"rabbitmq").unwrap();assert!(c["url"].starts_with("amqp://"));assert_eq!(c["port"],"5672");}
}

#[cfg(all(test,feature="desktop"))]mod live_recovery{
 use super::*;
 #[tokio::test]#[ignore="Explicit existing-resource deployment recovery; requires user authorization and LIARA_APPROVED_RECOVERY_RUN"]
 async fn resume_existing_approved_run(){
  let compat_repair=std::env::var("LIARA_APPROVED_COMPAT_REPAIR").as_deref()==Ok("1");
  let run_id=std::env::var("LIARA_APPROVED_RECOVERY_RUN").expect("Set LIARA_APPROVED_RECOVERY_RUN only for a user-authorized retry");
  let home=std::env::var("HOME").unwrap();let data=std::env::var("XDG_DATA_HOME").unwrap_or(format!("{home}/.local/share"));let store=Arc::new(Store::new(&std::path::Path::new(&data).join("ir.liara.launch.desktop")).unwrap());
  let mut run:DeploymentRun=store.get("run",&run_id).unwrap();
  assert_eq!(run.plan.revision,planner::revision(&run.plan));assert_ne!(run.status,"running");assert_eq!(run.plan.services.len(),2,"Recovery is limited to the two-resource sample");assert!(run.plan.services.iter().any(|s|s.name=="liara-two-service-demo-web"));
  let token=keyring::Entry::new("ir.liara.launch.desktop","liara-token").unwrap().get_password().unwrap();let team=keyring::Entry::new("ir.liara.launch.desktop","liara-team-id").unwrap().get_password().ok();let api=Liara::new(token,team).unwrap();let me=api.get("/v1/me").await.unwrap();assert_eq!(api.scoped_account_id(me["user"]["_id"].as_str().unwrap()),run.plan.account_id);
  assert!(run.network_id.is_some());assert!(!run.network_pending);let mut secrets=Secrets::new();
  for service in &run.plan.services{
   assert!(service.resize.is_none());assert!(service.volumes.is_empty(),"No new disks may be provisioned during this diagnostic");let r=run.resources.iter().find(|r|r.service_id==service.id).unwrap();let id=r.resource_id.as_deref().expect("Existing resource required");
   if service.kind=="app"{
    let detail=api.get(&format!("/v1/projects/{}",service.name)).await.unwrap();assert_eq!(detail["project"]["_id"].as_str(),Some(id));assert_eq!(detail["project"]["type"],service.platform);assert_eq!(detail["project"]["planID"],service.plan_id);
    println!("{}",json!({"status":detail["project"]["status"],"scale":detail["project"]["scale"],"bundle":detail["project"]["bundlePlanID"]}));
    let history=api.get(&format!("/v1/projects/{}/releases?page=1&count=100",service.name)).await.unwrap();if compat_repair{
     let previous=r.release_id.as_deref().expect("Existing failed compatibility release required");let release=api.get(&format!("/v1/releases/{previous}")).await.unwrap();assert_eq!(release["release"]["state"],"READY");let logs=api.get(&format!("/v1/projects/{}/logs",service.name)).await.unwrap();assert!(logs.to_string().contains("Please upgrade your client using:"),"Only the confirmed client compatibility failure may be repaired by this diagnostic");
    }else{assert!(empty_release_history(&history),"A release exists; reconcile rather than resubmit");assert!(r.release_id.is_none());}assert!(r.source_id.is_some());assert!(r.repair_attempts<2);
    assert_eq!(project::source_hash(&store.stage_dir(&run.plan.id,&service.id)).unwrap(),service.source_hash);
    let envs:BTreeMap<String,String>=detail["project"]["envs"].as_array().unwrap().iter().filter_map(|v|Some((v["key"].as_str()?.into(),v["value"].as_str()?.into()))).collect();assert!(service.env_keys.iter().all(|k|envs.contains_key(k)||service.bindings.contains_key(k)));secrets.insert(service.id.clone(),envs);
   }else{assert_eq!(service.kind,"database");let detail=api.get(&format!("/v1/databases/{id}")).await.unwrap();assert_eq!(detail["database"]["type"],service.platform);}
  }
  if compat_repair{let previous=run.resources.iter().find_map(|r|r.release_id.clone()).unwrap();run.events.push(RunEvent{at:now(),level:"info".into(),service_id:Some("web".into()),message:format!("User-authorized client compatibility repair; replacing nonfunctional release {previous} using the same approved source and resources.")});}
  for resource in &mut run.resources{if resource.request_pending.as_deref()==Some("release")||(compat_repair&&resource.release_id.is_some()){resource.request_pending=None;resource.release_id=None;resource.repair_attempts+=1;}}
  let mut redactions=vec![api.secret()];redactions.extend(secrets.values().flat_map(|m|m.values().cloned()));
  let executor=Executor{api,store:store.clone(),notify:Arc::new(|r|{if let Some(event)=r.events.last(){println!("{}: {}",event.level,event.message.chars().take(350).collect::<String>());}}),stop:Arc::new(AtomicBool::new(false)),secrets,redactions};
  executor.execute(run).await;let result:DeploymentRun=store.get("run",&run_id).unwrap();assert_eq!(result.status,"succeeded","{}",result.events.iter().rev().find(|e|e.level=="error").map(|e|e.message.as_str()).unwrap_or("Deployment was not verified"));
 }
}
