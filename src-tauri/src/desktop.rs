use crate::{models::*,agent,engine,liara::{self,Liara},planner,project,security::*,store::Store};
use serde_json::{json,Value};
use std::{collections::BTreeMap,sync::{Arc,Mutex,atomic::AtomicBool}};
use tauri::{Emitter,Manager,State};

struct Approved{request:ApprovalRequest,plan:DeploymentPlan,secrets:Secrets,resume:Option<String>,retry_releases:Vec<String>,resume_version:Option<(String,String)>} 
#[derive(Clone)]
struct Recovery{run_id:String,revision:String,updated_at:String}
struct PendingFix{review:crate::source_fix::FixReview,account_id:String,project:ProjectAnalysis}
struct Session{recoveries:BTreeMap<String,Recovery>,fixes:BTreeMap<String,PendingFix>,api:Option<Liara>,account:Option<Account>,approvals:BTreeMap<String,Approved>,active:BTreeMap<String,Arc<AtomicBool>>,domain_approvals:BTreeMap<String,Value>,run_secrets:BTreeMap<String,Secrets>,plan_envs:BTreeMap<String,Secrets>}
struct AppState{store:Arc<Store>,session:Mutex<Session>}
impl AppState {
 fn connected(&self)->Result<(Liara,Account)>{let s=self.session.lock().map_err(|_|"Session unavailable")?;Ok((s.api.clone().ok_or("Connect your Liara account first.")?,s.account.clone().ok_or("Connect your account first.")?))}
}
#[tauri::command]
fn app_status(state:State<AppState>)->Result<Value>{let s=state.session.lock().map_err(|_|"Session unavailable")?;Ok(json!({"agent":agent::status(),"account":s.account,"version":env!("CARGO_PKG_VERSION")}))}
#[tauri::command]
fn open_external(url:String)->Result<()>{
 let parsed=url::Url::parse(&url).map_err(|_|"Invalid external URL.")?;
 if parsed.scheme()!="https"{return Err("Only HTTPS links can be opened externally.".into());}
 tauri_plugin_opener::open_url(parsed.as_str(),None::<&str>).map_err(|e|format!("Could not open the system browser: {e}"))
}
#[tauri::command]
async fn connect_account(token:String,team_id:Option<String>,remember:bool,state:State<'_,AppState>)->Result<Account>{
 if token.trim().is_empty(){return Err("Enter your Liara API token.".into());}
 {let s=state.session.lock().map_err(|_|"Session unavailable")?;if !s.active.is_empty(){return Err("Stop the active deployment before switching accounts.".into());}}
 let team_id=team_id.map(|id|id.trim().to_string()).filter(|id|!id.is_empty());
 let api=Liara::new(token.trim().into(),team_id.clone())?;let mut account=api.account("session").await?;
 if remember {
  let token_saved=keyring::Entry::new("ir.liara.launch.desktop","liara-token").and_then(|entry|entry.set_password(token.trim())).is_ok();
  let team_saved=match team_id.as_deref(){Some(team)=>keyring::Entry::new("ir.liara.launch.desktop","liara-team-id").and_then(|entry|entry.set_password(team)).is_ok(),None=>delete_saved_credential("liara-team-id").is_ok()};
  if token_saved&&team_saved{account.credential_storage="keyring".into();}
  else{let _=delete_saved_credential("liara-token");account.warnings.push("System keyring is unavailable; the token and team context are stored in memory for this session only.".into());}
 }
 else{delete_saved_credential("liara-token")?;delete_saved_credential("liara-team-id")?;}
 let mut s=state.session.lock().map_err(|_|"Session unavailable")?;s.recoveries.clear();s.fixes.clear();s.api=Some(api);s.account=Some(account.clone());s.approvals.clear();s.run_secrets.clear();s.plan_envs.clear();Ok(account)
}
#[tauri::command]
async fn restore_account(state:State<'_,AppState>)->Result<Option<Account>>{
 let token=keyring::Entry::new("ir.liara.launch.desktop","liara-token").and_then(|e|e.get_password()).ok();
 if let Some(token)=token{let team_id=keyring::Entry::new("ir.liara.launch.desktop","liara-team-id").and_then(|e|e.get_password()).ok();connect_account(token,team_id,true,state).await.map(Some)}else{Ok(None)}
}
#[tauri::command]
async fn refresh_account(state:State<'_,AppState>)->Result<Account>{let (api,old)=state.connected()?;let account=api.account(&old.credential_storage).await?;state.session.lock().map_err(|_|"Session unavailable")?.account=Some(account.clone());Ok(account)}
#[tauri::command]
fn disconnect_account(state:State<AppState>)->Result<()>{
 let mut s=state.session.lock().map_err(|_|"Session unavailable")?;if !s.active.is_empty(){return Err("Stop the active deployment before disconnecting.".into());}
 delete_saved_credential("liara-token")?;delete_saved_credential("liara-team-id")?;
 s.recoveries.clear();s.fixes.clear();s.run_secrets.clear();s.plan_envs.clear();s.api=None;s.account=None;s.approvals.clear();s.domain_approvals.clear();Ok(())
}
fn delete_saved_credential(username:&str)->Result<()>{
 if let Ok(entry)=keyring::Entry::new("ir.liara.launch.desktop",username){entry.delete_credential().or_else(|e|if matches!(e,keyring::Error::NoEntry){Ok(())}else{Err(e)}).map_err(|_|"Could not update saved Liara credentials. Unlock the keyring and try again.")?;}
 Ok(())
}
#[tauri::command]
async fn scan_project(path:String,language:String,state:State<'_,AppState>)->Result<ProjectAnalysis>{
 let (_,account)=state.connected()?;
 let analysis=tauri::async_runtime::spawn_blocking(move||project::analyze(&path,&language)).await.map_err(|e|e.to_string())??;
 state.store.save("project",&analysis.id,&account.id,&analysis)?;state.store.show_project(&analysis.path,&account.id)?;Ok(analysis)
}
#[tauri::command]
fn list_projects(state:State<AppState>)->Result<Vec<ProjectAnalysis>>{let (_,a)=state.connected()?;state.store.list("project",&a.id)}
#[tauri::command]
fn remove_recent_project(project_id:String,state:State<AppState>)->Result<()>{let (_,account)=state.connected()?;state.store.hide_project(&project_id,&account.id)}
#[tauri::command]
fn delete_deployment(run_id:String,state:State<AppState>)->Result<()>{
 let mut session=state.session.lock().map_err(|_|"Session unavailable")?;
 let account=session.account.as_ref().ok_or("Connect your account first.")?;
 if session.active.contains_key(&run_id){return Err("Stop this deployment before removing its history.".into());}
 state.store.delete_run(&run_id,&account.id)?;
 session.run_secrets.remove(&run_id);session.approvals.retain(|_,a|a.resume.as_deref()!=Some(run_id.as_str()));session.domain_approvals.retain(|_,a|a["runId"].as_str()!=Some(run_id.as_str()));Ok(())
}
#[tauri::command]
async fn planning_questions(project_id:String,language:String,state:State<'_,AppState>)->Result<Vec<PlanningQuestion>>{
 let (_,account)=state.connected()?;
 let projects:Vec<ProjectAnalysis>=state.store.list("project",&account.id)?;
 let project=projects.iter().find(|p|p.id==project_id).ok_or("Select a project for this account first.")?;
 agent::questions(project,&language).await
}
#[tauri::command]
async fn prepare_plan(project_id:String,mut choices:PlanChoices,language:String,state:State<'_,AppState>)->Result<DeploymentPlan>{
 let (api,old)=state.connected()?;let mut account=api.account(&old.credential_storage).await?;
 let previous:ProjectAnalysis=state.store.get("project",&project_id)?;let path=previous.path.clone();let scan_language=language.clone();
 let mut project=tauri::async_runtime::spawn_blocking(move||project::analyze(&path,&scan_language)).await.map_err(|e|e.to_string())??;
 let recipe=if choices.guided{crate::recipes::grouped_django_nginx(&mut project,&language)?}else{None};
 if recipe.is_some(){crate::recipes::reuse_associated_resources(&state.store,&project,&account,&mut choices)?;}
 // Resolve the actual plan and all allocated disks before recommending an existing app upgrade.
 for detected in project.services.iter().filter(|s|s.kind=="app"){
  if let Some(existing)=choices.services.get(&detected.id).and_then(|s|s.existing_id.as_deref()){
   if let Some(index)=account.projects.iter().position(|p|p["_id"].as_str()==Some(existing)||p["project_id"].as_str()==Some(existing)){
    let name=account.projects[index]["project_id"].as_str().ok_or("Existing app name is unavailable.")?.to_string();
    let detail=api.get(&format!("/v1/projects/{name}")).await?;
    let app_id=detail["project"]["_id"].as_str().ok_or("Existing app ID unavailable.")?;
    let disks=api.get(&format!("/v1/projects/{app_id}/disks")).await?;
    let items=disks["disks"].as_array().ok_or("Existing disk allocations could not be verified.")?;
    let mut resource=account.projects[index].clone();resource["planID"]=detail["project"]["planID"].clone();
    // Internal planning annotation, never a field assumed to exist in the API response.
    resource["_launchDisks"]=json!(items.iter().map(sanitize_json).collect::<Vec<_>>());account.projects[index]=resource;
   }
  }
 }
 project.id=project_id.clone();state.store.save("project",&project.id,&account.id,&project)?;
 if choices.answers.len()>6||choices.answers.iter().any(|(k,v)|k.len()>80||v.len()>1000){return Err("Planning answers exceed the supported limits.".into());}
 let mut advice=agent::advise(&project,&account,&language,&choices).await?;
 if let Some(recipe)=recipe{let rec=advice.services.iter_mut().find(|s|s.service_id==recipe.service_id).ok_or("The agent omitted the grouped application. Retry planning.")?;for file in recipe.files{rec.files.retain(|f|f.path!=file.path);rec.files.push(file);}rec.port=Some(8080);rec.platform=Some("docker".into());}
 let plan=planner::build(&state.store,&project,&account,&choices,&advice,&language)?;
 let mut defaults=Secrets::new();
 for service in &plan.services{let ai=advice.services.iter().find(|s|s.service_id==service.id).map(|s|s.env_defaults.clone()).unwrap_or_default();defaults.insert(service.id.clone(),project::suggested_env(&service.env_keys,&ai,&format!("https://{}.liara.run",service.name),&service.platform));}
 state.session.lock().map_err(|_|"Session unavailable")?.plan_envs.insert(plan.id.clone(),defaults);
 state.store.save("plan",&plan.id,&account.id,&plan)?;state.session.lock().map_err(|_|"Session unavailable")?.account=Some(account);Ok(plan)
}
#[tauri::command]
async fn propose_project_fix(plan_id:String,run_id:Option<String>,language:String,state:State<'_,AppState>)->Result<crate::source_fix::FixReview>{
 let (_,account)=state.connected()?;let plan:DeploymentPlan=state.store.get("plan",&plan_id)?;
 if plan.account_id!=account.id{return Err("Plan belongs to a different account.".into());}
 let run=run_id.map(|id|state.store.get::<DeploymentRun>("run",&id)).transpose()?;
 if let Some(run)=&run{if run.plan.id!=plan.id||run.plan.revision!=plan.revision||run.plan.account_id!=account.id||!matches!(run.status.as_str(),"failed"|"paused"|"interrupted"){return Err("Only a stopped, unsuccessful deployment can be reviewed for project file changes.".into());}}
 if !state.session.lock().map_err(|_|"Session unavailable")?.active.is_empty(){return Err("Stop active deployments before reviewing project changes.".into());}
 let project:ProjectAnalysis=state.store.get("project",&plan.project_id)?;
 if run.is_none()&&plan.blockers.is_empty(){return Err("No deployment failure or planning blocker requires a project fix.".into());}
 let infrastructure=run.as_ref().and_then(|r|r.events.iter().rev().find(|e|e.level=="error")).is_some_and(|e|crate::source_fix::infrastructure_error(&e.message))||plan.blockers.iter().any(|b|b.contains("not_enough_storage_space"));
 let advice=if infrastructure{crate::source_fix::FixAdvice{explanation:if language=="fa"{"این مشکل از درخواست به لیارا، اتصال یا ظرفیت منابع است. تغییر کد پروژه آن را حل نمی‌کند. تنظیمات منابع و جزئیات خطا را بررسی کنید؛ برای کمبود فضا، پلن برنامه را در کنسول ارتقا دهید و استقرار را ادامه دهید."}else{"This failure concerns a Liara API request, connection or resource capacity. Editing application code will not resolve it. Review resources and error details; for insufficient storage, upgrade the app plan in Liara console and resume."}.into(),changes:vec![]}}else{agent::propose_source_fix(&project,&plan,run.as_ref(),&language).await?};
 let review=crate::source_fix::review(std::path::Path::new(&project.path),&plan,advice)?;
 let mut session=state.session.lock().map_err(|_|"Session unavailable")?;
 if session.account.as_ref().map(|a|&a.id)!=Some(&account.id){return Err("Account changed. Review the project fix again.".into());}
 session.fixes.clear();session.fixes.insert(review.id.clone(),PendingFix{review:review.clone(),account_id:account.id,project});Ok(review)
}
#[tauri::command]
fn apply_project_fix(approval_id:String,revision:String,state:State<AppState>)->Result<ProjectAnalysis>{
 let mut session=state.session.lock().map_err(|_|"Session unavailable")?;
 if !session.active.is_empty(){return Err("Stop active deployments before changing project files.".into());}
 let pending=session.fixes.remove(&approval_id).ok_or("Project fix approval is missing or already used.")?;
 let account=session.account.as_ref().ok_or("Connect your account first.")?;
 let plan:DeploymentPlan=state.store.get("plan",&pending.review.plan_id)?;
 if pending.account_id!=account.id||pending.review.revision!=revision||plan.revision!=revision||planner::revision(&plan)!=revision{return Err("Project fix does not match the reviewed plan or account.".into());}
 crate::source_fix::apply(std::path::Path::new(&pending.project.path),&pending.review,&state.store.data_dir.join("project-backups"))?;
 session.approvals.clear();session.fixes.clear();
 Ok(pending.project)
}
#[tauri::command]
async fn plan_environment(plan_id:String,state:State<'_,AppState>)->Result<Secrets>{
 let (api,account)=state.connected()?;let plan:DeploymentPlan=state.store.get("plan",&plan_id)?;
 if plan.account_id!=account.id{return Err("Plan belongs to a different account.".into());}
 let project:ProjectAnalysis=state.store.get("project",&plan.project_id)?;
 let mut result=state.session.lock().map_err(|_|"Session unavailable")?.plan_envs.get(&plan.id).cloned().unwrap_or_default();
 for service in &plan.services{
   let root=scoped(std::path::Path::new(&project.path),&service.root)?;
   let values:BTreeMap<String,String>=project::local_env(&root).into_iter().filter(|(key,value)|!value.trim().is_empty()&&service.env_keys.contains(key)&&!service.bindings.contains_key(key)).collect();
   let mut existing=BTreeMap::new();
   if service.kind=="app"&&service.existing_id.is_some(){let detail=api.get(&format!("/v1/projects/{}",service.name)).await?;if let Some(envs)=detail["project"]["envs"].as_array(){for env in envs{if let (Some(key),Some(value))=(env["key"].as_str(),env["value"].as_str()){if service.env_keys.iter().any(|k|k==key)&&!value.is_empty(){existing.insert(key.to_string(),value.to_string());}}}}}
   let entry=result.entry(service.id.clone()).or_insert_with(||project::suggested_env(&service.env_keys,&BTreeMap::new(),&format!("https://{}.liara.run",service.name),&service.platform));entry.extend(existing);entry.extend(values);crate::recipes::deployment_hosts(service,entry);entry.retain(|key,_|!service.bindings.contains_key(key));
 }
 state.session.lock().map_err(|_|"Session unavailable")?.plan_envs.insert(plan.id.clone(),result.clone());
 Ok(result)
}
#[tauri::command]
async fn prepare_storage_recovery(run_id:String,language:String,state:State<'_,AppState>)->Result<DeploymentPlan>{
 let (api,old)=state.connected()?;let run:DeploymentRun=state.store.get("run",&run_id)?;
 if run.plan.account_id!=old.id||run.status=="succeeded"{return Err("Select an unsuccessful deployment for this account.".into());}
 if !state.session.lock().map_err(|_|"Session unavailable")?.active.is_empty(){return Err("Stop active deployments before reviewing a plan upgrade.".into());}
 let account=api.account(&old.credential_storage).await?;
 let mut plan=run.plan.clone();plan.id=id();plan.created_at=now();plan.language=language.clone();plan.blockers.retain(|b|!b.contains("not_enough_storage_space"));
 plan.network=run.network_id.clone().or(plan.network);
 for service in &mut plan.services{
  let progress=run.resources.iter().find(|r|r.service_id==service.id).ok_or("Saved resource progress is missing.")?;
  if progress.resource_id.is_some(){service.existing_id=progress.resource_id.clone();}
  if service.kind!="app"||!service.volumes.iter().any(|v|v.resolution.as_deref()==Some("disk")){continue;}
  let detail=api.get(&format!("/v1/projects/{}",service.name)).await?;
  if detail["project"]["_id"].as_str()!=progress.resource_id.as_deref()||detail["project"]["type"].as_str()!=Some(&service.platform){return Err("The existing app no longer matches this deployment. Review the resources again.".into());}
  let current_id=detail["project"]["planID"].as_str().ok_or("Current application plan unavailable.")?;
  let current=account.plans.iter().find(|p|p.category=="app"&&p.id==current_id&&p.bundle_id==service.bundle_id).ok_or("Current plan pricing is unavailable. Refresh the live catalog.")?;
  let disks=api.get(&format!("/v1/projects/{}/disks",progress.resource_id.as_deref().unwrap())).await?;
  let required=crate::storage::required_storage(&service.volumes,disks["disks"].as_array().ok_or("Disk list could not be verified.")?)?;
  let target=crate::storage::fitting_plan(&account.plans,current,&service.platform,required).ok_or_else(||if language=="fa"{format!("برای {} پلن سازگار با فضای کافی و قیمت تاییدشده پیدا نشد. هیچ تغییری انجام نشد.",service.name)}else{format!("No compatible, priced plan has enough storage for {}. No changes were made.",service.name)})?;
  // Keep an already sufficient current plan; recovery must never silently downgrade it.
  let target=if current.disk>=required{current}else{target};
  service.resize=if target.id!=current.id{Some(crate::storage::resize_advice(current,target,required)?)}else{None};
  service.plan_id=target.id.clone();service.bundle_id=target.bundle_id.clone();service.monthly_price=target.monthly_price;
  if service.resize.is_some(){service.rationale=crate::storage::explain(service,&language);}
 }
 // Preserve the approved snapshot, rather than rereading files that may have changed.
 for service in &plan.services{let progress=run.resources.iter().find(|r|r.service_id==service.id).unwrap();if service.kind=="app"&&service.image.is_none()&&progress.source_id.is_none(){
  let source=state.store.stage_dir(&run.plan.id,&service.id);
  if project::source_hash(&source)?!=service.source_hash{return Err("The approved staging snapshot changed. Prepare a fresh deployment plan.".into());}
  let staged=project::stage(&source,&state.store.stage_dir(&plan.id,&service.id),&[])?;
  if staged!=service.source_hash{return Err("The recovery staging snapshot does not match the approved source.".into());}
 }}
 plan.summary=if language=="fa"{"پلن مناسب مجموع فضای فایل‌ها انتخاب شد. هزینه را تایید کنید تا برنامه موجود ارتقا پیدا کند و استقرار ادامه یابد."}else{"We selected a plan for all your saved files. Approve its cost to upgrade the existing app and continue deployment."}.into();
 plan.revision=planner::revision(&plan);state.store.save("plan",&plan.id,&account.id,&plan)?;
 let mut session=state.session.lock().map_err(|_|"Session unavailable")?;
 if session.account.as_ref().map(|a|&a.id)!=Some(&account.id){return Err("Account changed. Request a new recommendation.".into());}
 if session.recoveries.len()>50{session.recoveries.clear();}session.recoveries.insert(plan.id.clone(),Recovery{run_id,revision:run.plan.revision.clone(),updated_at:run.updated_at.clone()});
 if let Some(defaults)=session.plan_envs.get(&run.plan.id).cloned(){session.plan_envs.insert(plan.id.clone(),defaults);}
 session.account=Some(account);Ok(plan)
}
#[tauri::command]
async fn request_approval(plan_id:String,mut secrets:Secrets,resume_id:Option<String>,state:State<'_,AppState>)->Result<ApprovalRequest>{
 let (api,account)=state.connected()?;let plan:DeploymentPlan=state.store.get("plan",&plan_id)?;
 if plan.account_id!=account.id{return Err("Plan belongs to a different account.".into());}
 let resume=resume_id.as_ref().map(|id|state.store.get::<DeploymentRun>("run",id)).transpose()?;
 if let Some(run)=&resume{
  let session=state.session.lock().map_err(|_|"Session unavailable")?;
  let recovery=session.recoveries.get(&plan.id).is_some_and(|r|r.run_id==run.id&&r.revision==run.plan.revision&&r.updated_at==run.updated_at);
  if run.plan.account_id!=account.id||run.plan.project_id!=plan.project_id||(!recovery&&(run.plan.id!=plan.id||run.plan.revision!=plan.revision)){return Err("Run does not match the plan.".into());}
  if run.status=="succeeded"||session.active.contains_key(&run.id){return Err("This deployment cannot be resumed while active or after success.".into());}
 }
 let cached=if let Some(id)=&resume_id{state.session.lock().map_err(|_|"Session unavailable")?.run_secrets.get(id).cloned().unwrap_or_default()}else{Secrets::new()};
 let project:ProjectAnalysis=state.store.get("project",&plan.project_id)?;
 let mut required=plan.clone();
 for service in &mut required.services {
    if resume.as_ref().is_some_and(|run|run.resources.iter().any(|r|r.service_id==service.id&&r.phase=="verified")){service.env_keys.clear();continue;}
    let values=secrets.entry(service.id.clone()).or_default();
    if let Some(saved)=cached.get(&service.id){for (key,value) in saved{values.entry(key.clone()).or_insert_with(||value.clone());}}
    // Recover values already applied to Liara after an app restart. Never log or persist them.
    if resume.as_ref().is_some_and(|run|run.resources.iter().any(|r|r.service_id==service.id&&r.resource_id.is_some()))&&service.kind=="app"{
      let detail=api.get(&format!("/v1/projects/{}",service.name)).await?;
      if let Some(envs)=detail["project"]["envs"].as_array(){for env in envs{if let (Some(key),Some(value))=(env["key"].as_str(),env["value"].as_str()){if service.env_keys.iter().any(|k|k==key){values.entry(key.into()).or_insert_with(||value.into());}}}}
    }
    let root=scoped(std::path::Path::new(&project.path),&service.root)?;
    let local=project::local_env(&root);
    let values=secrets.entry(service.id.clone()).or_default();
    for key in &service.env_keys {if !service.bindings.contains_key(key)&&!values.contains_key(key){if let Some(v)=local.get(key).filter(|v|!v.trim().is_empty()){values.insert(key.clone(),v.clone());}}}
 }
 let defaults=state.session.lock().map_err(|_|"Session unavailable")?.plan_envs.get(&plan.id).cloned().unwrap_or_default();
 for service in &required.services{let values=secrets.entry(service.id.clone()).or_default();let mut automatic=project::suggested_env(&service.env_keys,&BTreeMap::new(),&format!("https://{}.liara.run",service.name),&service.platform);if let Some(d)=defaults.get(&service.id){automatic.extend(d.clone());}for (key,value) in automatic{if !service.bindings.contains_key(&key){values.entry(key).or_insert(value);}}}
 planner::validate_secrets(&required,&secrets)?;

 let mut request=planner::approval(&plan)?;
 let retry_releases=resume.as_ref().map(engine::release_retry_candidates).unwrap_or_default();
 if let Some(run)=&resume{for service_id in &retry_releases{
  engine::verify_release_retry(&api,run,service_id).await?;
  let service=plan.services.iter().find(|s|s.id==*service_id).ok_or("Unknown service")?;
  request.actions.push(if plan.language=="fa"{format!("لیارا هیچ نسخه‌ای برای {} ثبت نکرده است. ارسال نسخه را با منابع و فایل بارگذاری‌شدهٔ موجود دوباره انجام دهید؛ منبع تازه‌ای ساخته نمی‌شود.",service.name)}else{format!("Liara records no releases for {}. Retry release submission using existing resources and uploaded source; no new resources will be created.",service.name)});
 }}
 let mut s=state.session.lock().map_err(|_|"Session unavailable")?;s.approvals.clear();s.approvals.insert(request.id.clone(),Approved{request:request.clone(),plan,secrets,resume:resume_id,retry_releases,resume_version:resume.as_ref().map(|r|(r.plan.revision.clone(),r.updated_at.clone()))});Ok(request)
}
#[tauri::command]
async fn start_deployment(approval_id:String,revision:String,app:tauri::AppHandle,state:State<'_,AppState>)->Result<DeploymentRun>{
 let (api,account)=state.connected()?;
 let approved={let mut s=state.session.lock().map_err(|_|"Session unavailable")?;if !s.active.is_empty(){return Err("A deployment is already running.".into());}s.approvals.remove(&approval_id).ok_or("Approval is missing or already used.")?};
 if approved.request.revision!=revision||approved.plan.revision!=planner::revision(&approved.plan){return Err("Approval does not match this plan revision.".into());}
 if approved.plan.account_id!=account.id||approved.request.expires_at<now(){return Err("Approval expired or account changed. Review the plan again.".into());}
 let mut run=if let Some(ref id)=approved.resume {state.store.get::<DeploymentRun>("run",id)?}else{DeploymentRun{id:id(),plan:approved.plan.clone(),status:"running".into(),started_at:now(),updated_at:now(),network_id:None,network_pending:false,resources:approved.plan.services.iter().map(|s|ResourceProgress{service_id:s.id.clone(),phase:"pending".into(),..Default::default()}).collect(),events:vec![]}};
 if run.plan.account_id!=account.id||run.plan.project_id!=approved.plan.project_id||approved.resume_version.as_ref().is_some_and(|(revision,updated)|*revision!=run.plan.revision||*updated!=run.updated_at){return Err("Run changed after approval. Review recovery again.".into());}
 run.plan=approved.plan.clone();
 for service_id in &approved.retry_releases{engine::verify_release_retry(&api,&run,service_id).await?;}
 for resource in &mut run.resources{if approved.retry_releases.contains(&resource.service_id){resource.request_pending=None;}}
 run.status="running".into();state.store.save("run",&run.id,&account.id,&run)?;
 state.session.lock().map_err(|_|"Session unavailable")?.run_secrets.insert(run.id.clone(),approved.secrets.clone());
 let stop=Arc::new(AtomicBool::new(false));state.session.lock().map_err(|_|"Session unavailable")?.active.insert(run.id.clone(),stop.clone());
 let emit_app=app.clone();let store=state.store.clone();let mut redactions=vec![api.secret()];redactions.extend(approved.secrets.values().flat_map(|m|m.values().cloned()));
 let executor=engine::Executor{api,store,notify:Arc::new(move|r|{let _=emit_app.emit("deployment:update",r);}),stop,secrets:approved.secrets,redactions};
 let to_run=run.clone();let run_id=run.id.clone();
 tauri::async_runtime::spawn(async move {executor.execute(to_run).await;let state=app.state::<AppState>();if let Ok(mut s)=state.session.lock(){s.active.remove(&run_id);};});
 Ok(run)
}
#[tauri::command]
fn stop_deployment(run_id:String,state:State<AppState>)->Result<()>{let s=state.session.lock().map_err(|_|"Session unavailable")?;if let Some(stop)=s.active.get(&run_id){stop.store(true,std::sync::atomic::Ordering::Relaxed);}Ok(())}
#[tauri::command]
fn list_runs(state:State<AppState>)->Result<Vec<DeploymentRun>>{let (_,a)=state.connected()?;let mut runs:Vec<DeploymentRun>=state.store.list("run",&a.id)?;let s=state.session.lock().map_err(|_|"Session unavailable")?;for r in &mut runs{if r.status=="running"&&!s.active.contains_key(&r.id){r.status="interrupted".into();}}Ok(runs)}
#[tauri::command]
async fn chat_agent(project_id:Option<String>,run_id:Option<String>,messages:Vec<ChatMessage>,language:String,state:State<'_,AppState>)->Result<String>{
 let (_,account)=state.connected()?;
 let project=project_id.map(|id|state.store.get::<ProjectAnalysis>("project",&id)).transpose()?;
 let run=run_id.map(|id|state.store.get::<DeploymentRun>("run",&id)).transpose()?;
 if run.as_ref().is_some_and(|r|r.plan.account_id!=account.id){return Err("Run belongs to another account.".into());}
 agent::chat(project.as_ref(),Some(&account),run.as_ref(),&messages,&language).await
}
#[tauri::command]
async fn prepare_domain(run_id:String,service_id:String,domain:String,manage_dns:bool,state:State<'_,AppState>)->Result<Value>{
 let (api,account)=state.connected()?;let run:DeploymentRun=state.store.get("run",&run_id)?;
 if run.plan.account_id!=account.id{return Err("Run belongs to another account.".into());}
 let name=domain.trim().to_lowercase();
 if !regex::Regex::new(r"^(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z]{2,63}$").unwrap().is_match(&name){return Err("Enter a domain such as www.example.com, without a URL path.".into());}
 let resource=run.resources.iter().find(|r|r.service_id==service_id).ok_or("Unknown service")?;
 let project=api.get(&format!("/v1/projects/{}",resource.resource_id.as_deref().ok_or("Deploy the application first")?)).await?;
 let project_id=project["project"]["_id"].as_str().ok_or("Application ID unavailable")?;
 let project_name=project["project"]["project_id"].as_str().ok_or("Application name unavailable")?;
 let domains=api.get("/v1/domains").await?;
 let existing=domains["domains"].as_array().and_then(|a|a.iter().find(|d|d["name"].as_str()==Some(&name)));
 if existing.is_some_and(|d|d["project"]["_id"].as_str()!=Some(project_id)){return Err("This domain is assigned elsewhere. Move it explicitly in the Liara console before continuing.".into());}
 let mut zone=Value::Null;
 if manage_dns {
    let zones=api.request(reqwest::Method::GET,liara::DNS,"/api/v1/zones",None).await?;
    let list=zones["data"].as_array().or_else(||zones["zones"].as_array()).ok_or("DNS zone list format is unsupported. Use the external DNS instructions.")?;
    zone=list.iter().filter(|z|z["name"].as_str().is_some_and(|n|name==n||name.ends_with(&format!(".{n}")))).max_by_key(|z|z["name"].as_str().unwrap_or("").len()).cloned().ok_or("No matching Liara DNS zone. Create/import the zone in the console, or use external DNS instructions.")?;
 }
 let request=json!({"id":id(),"accountId":account.id,"runId":run_id,"domain":name,"projectId":project_id,"projectName":project_name,"domainId":existing.and_then(|d|d["_id"].as_str()),"manageDns":manage_dns,"zone":zone,"expiresAt":(chrono::Utc::now()+chrono::Duration::minutes(15)).to_rfc3339(),"actions":["Attach this domain to the selected application","Create a CNAME record only if no conflicting record exists (when selected)","Check DNS and request TLS when verification succeeds"]});
 state.session.lock().map_err(|_|"Session unavailable")?.domain_approvals.insert(request["id"].as_str().unwrap().into(),request.clone());Ok(request)
}
#[tauri::command]
async fn apply_domain(approval_id:String,state:State<'_,AppState>)->Result<Value>{
 let (api,account)=state.connected()?;let r=state.session.lock().map_err(|_|"Session unavailable")?.domain_approvals.remove(&approval_id).ok_or("Domain approval is missing or already consumed.")?;
 if r["accountId"].as_str()!=Some(&account.id)||r["expiresAt"].as_str().unwrap_or("")<now().as_str(){return Err("Domain approval expired or account changed.".into());}
 let domain=r["domain"].as_str().unwrap();
 let id=if let Some(id)=r["domainId"].as_str(){id.to_string()}else{let result=api.post("/v1/domains",json!({"name":domain,"project":r["projectId"],"type":"PROJECT"})).await?;result["domain"]["_id"].as_str().ok_or("Domain creation returned no ID. Check the console before retrying.")?.into()};
 let check=api.get(&format!("/v1/domains/{id}/check")).await?;
 let target=check["domain"]["CNameRecord"].as_str().ok_or("Liara did not provide the required DNS target. Check the domain in the console.")?;
 let mut dns_message=String::from("Create the displayed CNAME at your DNS provider, then check again. For an apex domain, use your provider's ALIAS/flattening instructions. Registrar nameserver changes must be completed by you.");
 if r["manageDns"].as_bool()==Some(true){
    let zone=r["zone"]["name"].as_str().ok_or("Zone name unavailable")?;
    let record_name=domain.strip_suffix(&format!(".{zone}")).unwrap_or("@");
    if record_name=="@"{dns_message="For an apex domain, use Liara's console ALIAS configuration; automatic CNAME creation is limited to subdomains.".into();}
    else {
      let records=api.request(reqwest::Method::GET,liara::DNS,&format!("/api/v1/zones/{zone}/dns-records"),None).await?;
      let list=records["data"].as_array().or_else(||records["records"].as_array()).ok_or("Unsupported DNS response; no record was changed.")?;
      if list.iter().any(|x|x["name"].as_str()==Some(record_name)){dns_message="A record with this name already exists; it was preserved. Review it in the DNS console.".into();}
      else{api.request(reqwest::Method::POST,liara::DNS,&format!("/api/v1/zones/{zone}/dns-records"),Some(json!({"name":record_name,"type":"CNAME","ttl":300,"contents":[{"content":target}]}))).await?;dns_message="CNAME record created. DNS propagation may take time.".into();}
    }
 }
 // A fresh check determines whether TLS can be requested; never infer verification from successful creation.
 let check=api.get(&format!("/v1/domains/{id}/check")).await?;
 let status=check["domain"]["status"].as_str().unwrap_or("pending");
 let tls=if matches!(status.to_lowercase().as_str(),"verified"|"active"|"connected"){match api.post("/v1/domains/provision-ssl-certs",json!({"domain":id})).await{Ok(v)=>json!({"requested":true,"response":v}),Err(e)=>json!({"requested":false,"error":e})}}else{json!({"requested":false,"reason":"DNS verification pending"})};
 let result=json!({"domain":domain,"domainId":id,"target":target,"status":status,"instructions":dns_message,"tls":tls});
 state.store.save("domain",&id,&account.id,&result)?;Ok(result)
}
pub fn run(){
 if cfg!(debug_assertions){let _=dotenvy::from_path(concat!(env!("CARGO_MANIFEST_DIR"),"/../.env"));}
 tauri::Builder::default().plugin(tauri_plugin_dialog::init()).plugin(tauri_plugin_opener::init()).setup(|app|{
   let dir=app.path().app_data_dir()?;let store=Arc::new(Store::new(&dir).map_err(std::io::Error::other)?);
   app.manage(AppState{store,session:Mutex::new(Session{recoveries:BTreeMap::new(),fixes:BTreeMap::new(),api:None,account:None,approvals:BTreeMap::new(),active:BTreeMap::new(),domain_approvals:BTreeMap::new(),run_secrets:BTreeMap::new(),plan_envs:BTreeMap::new()})});Ok(())
 }).invoke_handler(tauri::generate_handler![app_status,open_external,connect_account,restore_account,refresh_account,disconnect_account,scan_project,list_projects,remove_recent_project,delete_deployment,planning_questions,prepare_plan,propose_project_fix,apply_project_fix,plan_environment,prepare_storage_recovery,request_approval,start_deployment,stop_deployment,list_runs,chat_agent,prepare_domain,apply_domain]).run(tauri::generate_context!()).expect("Unable to launch Liara desktop application");
}
