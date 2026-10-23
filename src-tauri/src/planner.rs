use crate::{models::*,project,security::*,agent::Advice,store::Store};
use std::{collections::BTreeSet,path::Path};

fn localized<'a>(language:&str,en:&'a str,fa:&'a str)->String{if language=="fa"{fa}else{en}.into()}
fn docker_command(command:&serde_json::Value)->Option<String>{
 if let Some(items)=command.as_array(){let args:Option<Vec<&str>>=items.iter().map(|v|v.as_str()).collect();return args.map(|args|format!("CMD {}",serde_json::to_string(&args).unwrap_or_default()));}
 command.as_str().map(|s|format!("CMD {}",serde_json::to_string(&["/bin/sh","-c",s]).unwrap_or_default()))
}

pub fn revision(plan:&DeploymentPlan)->String{let mut copy=plan.clone();copy.revision.clear();hash(&serde_json::to_vec(&copy).unwrap_or_default())}
pub fn build(store:&Store,project:&ProjectAnalysis,account:&Account,choices:&PlanChoices,advice:&Advice,language:&str)->Result<DeploymentPlan>{
 let mut plan=DeploymentPlan{id:id(),project_id:project.id.clone(),account_id:account.id.clone(),revision:String::new(),network:choices.network.clone().or_else(||account.networks.first().and_then(|n|n["_id"].as_str().map(str::to_string))).or_else(||Some(format!("new:{}-network",slug(&project.name)))),services:vec![],summary:advice.summary.clone(),warnings:project.warnings.clone(),blockers:vec![],created_at:now(),language:language.into(),allow_config_repairs:choices.allow_config_repairs};
 plan.warnings.extend(advice.warnings.clone());
 let root=Path::new(&project.path);
 if project.services.is_empty(){plan.blockers.push("No deployable services detected.".into());}
 for detected in &project.services {
   let mut choice=choices.services.get(&detected.id).cloned().unwrap_or_default();
   let previous_bundle=choice.bundle_id.clone();
   let rec=advice.services.iter().find(|s|s.service_id==detected.id);
   if choices.guided {if let Some(rec)=rec{
     if !choice.plan_locked{choice.plan_id=rec.plan_id.clone();choice.bundle_id=rec.bundle_id.clone();}
     for (id,volume) in &rec.volumes{if detected.volumes.iter().any(|v|v.id==*id)&&volume.resolution.as_deref()!=Some("temporary"){if choice.volumes.get(id).is_none_or(|v|v.resolution.is_none()){let mut recommended=volume.clone();if recommended.resolution.as_deref()==Some("copy")&&!detected.volumes.iter().any(|v|v.id==*id&&v.kind=="bind"&&v.read_only){recommended.resolution=None;}choice.volumes.insert(id.clone(),recommended);}}}
   }}
   if choices.guided {for (i,volume) in detected.volumes.iter().enumerate(){
     let current=choice.volumes.get(&volume.id);
     let missing=current.is_none_or(|v|v.resolution.is_none());
     let copy_missing=current.is_some_and(|v|v.resolution.as_deref()==Some("copy"))&&volume.kind=="bind"&&!scoped(root,&volume.source).ok().is_some_and(|p|p.exists())&&Path::new(&volume.source).extension().is_none();
     if missing||copy_missing{
       let can_copy=volume.kind=="bind"&&volume.read_only&&(Path::new(&volume.source).extension().is_some()||scoped(root,&volume.source).ok().is_some_and(|p|p.exists()));
       choice.volumes.insert(volume.id.clone(),VolumeChoice{resolution:Some(if can_copy{"copy"}else{"disk"}.into()),disk_name:if can_copy{None}else{Some(format!("{}-files-{}",slug(&detected.id).chars().take(30).collect::<String>(),i+1))},size_gb:if can_copy{None}else{Some(10)}});
       if !can_copy{plan.warnings.push(localized(language,"A 10 GB starting file allocation was selected because a specific storage estimate was unavailable. You can change it in deployment settings.","برای فایل‌ها ۱۰ گیگابایت فضای اولیه انتخاب شد چون تخمین دقیق در دسترس نبود. در تنظیمات استقرار می‌توانید آن را تغییر دهید."));}
     }
   }}
   let supported_apps=["node","next","react","angular","vue","static","python","django","flask","php","laravel","go","dotnet","docker"];
   let suggested=rec.and_then(|a|a.platform.clone()).filter(|p|detected.kind=="app"&&supported_apps.contains(&p.as_str()));
   let platform=suggested.unwrap_or(detected.platform.clone());
   if detected.kind=="app" && !["node","next","react","angular","vue","static","python","django","flask","php","laravel","go","dotnet","docker"].contains(&platform.as_str()){return Err(format!("Unsupported deployment platform: {platform}"));}
   let available:Vec<_>=account.plans.iter().filter(|p|p.category==detected.kind && (p.supports.is_empty()||p.supports.contains(&platform))).collect();
   let recommended=available.iter().copied().find(|p|p.ram>=if platform=="next"{1.0}else{0.5}&&p.monthly_price.is_some()).or_else(||available.first().copied());
   let mut selected=if let Some(ref wanted)=choice.plan_id {available.iter().copied().find(|p|p.id==*wanted&&choice.bundle_id.as_ref().is_none_or(|b|*b==p.bundle_id))}else{recommended};
   if selected.is_none()&&choice.existing_id.is_none(){plan.blockers.push(format!("{}: no compatible live plan selected.",detected.name));}
   let name=choice.name.clone().unwrap_or_else(||format!("{}-{}",slug(&project.name),slug(&detected.id)));
   if !safe_name(&name){plan.blockers.push(format!("Invalid service name: {name}. Use 3–50 lowercase letters, numbers and hyphens."));}
   let existing=choice.existing_id.clone().filter(|s|!s.is_empty());
   let resources=match detected.kind.as_str(){"database"=>&account.databases,"bucket"=>&account.buckets,_=>&account.projects};
   if let Some(ref resource)=existing {
      let found=resources.iter().find(|r|["_id","DBId","id","project_id"].iter().any(|k|r[k].as_str()==Some(resource)));
      if let Some(r)=found {if let Some(kind)=r["type"].as_str(){if kind!=platform{plan.blockers.push(format!("{}: existing resource platform differs from {platform}.",detected.name));}}}
      else{plan.blockers.push(format!("{}: selected existing resource is unavailable.",detected.name));}
   }else if resources.iter().any(|r|["project_id","hostname","name"].iter().any(|k|r[k].as_str()==Some(&name))){plan.blockers.push(format!("{name} already exists. Select that resource explicitly or choose another name."));}
   let mut files=rec.map(|r|r.files.clone()).unwrap_or_default();
   let dir=scoped(root,&detected.root)?;
   if detected.kind=="app"&&platform=="docker"&&detected.image.is_none()&&!dir.join("Dockerfile").exists()&&!files.iter().any(|f|f.path=="Dockerfile"){plan.blockers.push(format!("{name}: Docker deployment requires a Dockerfile."));}
   for warning in &detected.warnings {if warning.contains("blocked")||warning.contains("unsupported"){plan.blockers.push(format!("{name}: {warning}"));}else{plan.warnings.push(format!("{name}: {warning}"));}}
   let mut volumes=detected.volumes.clone();
   for volume in &mut volumes {
      let selected=choice.volumes.get(&volume.id);
      if volume.kind=="bind"&&volume.source=="." {volume.resolution=Some("copy".into());continue;}
      volume.resolution=selected.and_then(|v|v.resolution.clone()).or_else(||if volume.read_only&&volume.kind=="bind"{Some("copy".into())}else{None});
      volume.disk_name=selected.and_then(|v|v.disk_name.clone());
      volume.size_gb=selected.and_then(|v|v.size_gb);
      match volume.resolution.as_deref(){
       None=>plan.blockers.push(format!("{}: {}",name,localized(language,"Choose how to handle Compose volume","برای هر فضای ذخیره‌سازی Compose یک روش انتخاب کنید."))),
       Some("copy")=>{
        if volume.kind!="bind"||!volume.read_only {plan.blockers.push(format!("{}: {}",name,localized(language,"Only a read-only project bind mount can be copied into the image.","فقط اتصال محلیِ فقط‌خواندنی را می‌توان در تصویر کپی کرد.")));}
        else if volume.source.is_empty()||scoped(root,&volume.source).is_err()||!scoped(root,&volume.source).ok().is_some_and(|p|p.exists()) {plan.blockers.push(format!("{}: {} {}",name,localized(language,"Read-only bind source is missing or outside the project:","مسیر اتصال محلی پیدا نشد یا بیرون از پروژه است:"),volume.source));}
       },
       Some("disk")=>{
        if !volume.disk_name.as_deref().is_some_and(|n|safe_name(n)){plan.blockers.push(format!("{}: {}",name,localized(language,"Enter a valid name for the Liara disk.","نام معتبری برای دیسک لیارا وارد کنید.")));}
        if !volume.size_gb.is_some_and(|n|(1..=1000).contains(&n)){plan.blockers.push(format!("{}: {}",name,localized(language,"Set the disk size from 1 to 1000 GB.","اندازه دیسک را بین ۱ تا ۱۰۰۰ گیگابایت تعیین کنید.")));}
       },
       Some("temporary")=>plan.warnings.push(format!("{} · {} {}",name,localized(language,"Temporary Compose storage at","فضای ذخیره‌سازی موقت در مسیر"),volume.target)),
       _=>plan.blockers.push(format!("{}: {}",name,localized(language,"Choose a supported storage option.","یک روش پشتیبانی‌شده برای ذخیره‌سازی انتخاب کنید."))),
      }
   }
   if volumes.iter().any(|v|v.resolution.as_deref()==Some("temporary")){plan.warnings.push(localized(language,"Temporary container storage can be lost on restart or redeploy and is not shared between separate Liara apps.","فضای موقت کانتینر ممکن است پس از راه‌اندازی دوباره یا استقرار مجدد از بین برود و بین برنامه‌های جداگانه لیارا مشترک نیست."));}
   let mut image=detected.image.clone();
   if detected.kind=="app"&&image.is_some()&&!volumes.is_empty(){
      let mut dockerfile=format!("FROM {}\n",image.as_deref().unwrap());
      for volume in volumes.iter().filter(|v|v.resolution.as_deref()==Some("copy")){
        let source=volume.source.trim_start_matches("./");
        if !source.is_empty(){dockerfile.push_str(&format!("COPY {}\n",serde_json::to_string(&[source,volume.target.as_str()]).unwrap_or_default()));}
      }
      if let Some(cmd)=detected.command.as_ref().and_then(docker_command){dockerfile.push_str(&format!("{cmd}\n"));}
      files.retain(|f|f.path!="Dockerfile");files.push(GeneratedFile{path:"Dockerfile".into(),content:dockerfile,reason:localized(language,"Compose image and read-only configuration staged for a Liara build","تصویر Compose و تنظیمات فقط‌خواندنی برای ساخت در لیارا آماده شد")});
      image=None;
   }else if detected.kind=="app"&&image.is_none()&&detected.command.as_ref().is_some_and(|v|!v.is_null()){
      let dockerfile_path=dir.join("Dockerfile");
      if let Ok(existing)=std::fs::read_to_string(&dockerfile_path){
       let mut lines:Vec<String>=existing.lines().filter(|line|!line.trim_start().to_ascii_uppercase().starts_with("CMD ")).map(str::to_string).collect();
       if let Some(cmd)=detected.command.as_ref().and_then(docker_command){lines.push(cmd);}
       files.retain(|f|f.path!="Dockerfile");files.push(GeneratedFile{path:"Dockerfile".into(),content:format!("{}\n",lines.join("\n")),reason:localized(language,"Compose command staged in the deployment Dockerfile","دستور Compose در Dockerfile موقت ثبت شد")});
      }
   }
   for volume in &volumes {if volume.kind!="bind"&&!volume.source.is_empty()&&project.services.iter().flat_map(|s|s.volumes.iter()).filter(|v|v.kind!="bind"&&v.source==volume.source).count()>1 {plan.warnings.push(format!("{}: {} {}",name,localized(language,"Compose volume is used by multiple separate apps; this plan does not preserve cross-app file sharing:","این فضای Compose میان چند برنامه جداگانه استفاده شده است؛ این برنامه اشتراک فایل میان آن‌ها را حفظ نمی‌کند:"),volume.source));}}
   if choices.guided&&volumes.iter().any(|v|v.resolution.as_deref()==Some("disk")&&!v.source.is_empty()&&project.services.iter().filter(|other|other.id!=detected.id).any(|other|other.volumes.iter().any(|x|x.kind==v.kind&&x.source==v.source))){
     plan.blockers.push(localized(language,"This project shares saved files between services. Separate disks cannot preserve that sharing. Review the file-sharing design before deploying.","این پروژه فایل‌های مشترک بین چند بخش دارد. دیسک‌های جداگانه این اشتراک را حفظ نمی‌کنند؛ قبل از استقرار باید روش اشتراک فایل بررسی شود."));
   }
   let mut bindings=choice.bindings.clone();
   for key in &detected.env_keys {
      if bindings.contains_key(key){continue;}
      let dbs:Vec<_>=project.services.iter().filter(|s|s.kind=="database"&&(detected.dependencies.contains(&s.id)||project.services.iter().filter(|s|s.kind=="database").count()==1)).collect();
      let rabbit=key.contains("RABBITMQ")||key.starts_with("CELERY_BROKER");
      let db=if rabbit{project.services.iter().find(|s|s.kind=="database"&&s.platform=="rabbitmq")}else if key.contains("REDIS"){dbs.iter().copied().find(|s|s.platform=="redis")}else if key.contains("MONGO"){dbs.iter().copied().find(|s|s.platform=="mongodb")}else{dbs.iter().copied().find(|s|s.platform!="redis")};
      if let Some(db)=db {
        let field=match key.as_str(){"DATABASE_URL"|"MONGODB_URI"|"MONGO_URL"|"REDIS_URL"|"CELERY_BROKER_URL"=>Some("url"),"POSTGRES_HOST"|"PGHOST"|"RABBITMQ_HOST"|"DB_HOST"|"DATABASE_HOST"|"REDIS_HOST"=>Some("host"),"POSTGRES_PORT"|"PGPORT"|"RABBITMQ_PORT"|"DB_PORT"|"DATABASE_PORT"|"REDIS_PORT"=>Some("port"),"POSTGRES_PASSWORD"|"PGPASSWORD"|"RABBITMQ_PASS"|"RABBITMQ_PASSWORD"|"DB_PASSWORD"|"DATABASE_PASSWORD"|"REDIS_PASSWORD"=>Some("password"),"POSTGRES_USER"|"PGUSER"|"RABBITMQ_USER"|"RABBITMQ_USERNAME"|"DB_USERNAME"|"DB_USER"|"DATABASE_USER"=>Some("username"),"POSTGRES_DB"|"PGDATABASE"|"DB_DATABASE"|"DB_NAME"|"DATABASE_NAME"=>Some("database"),_=>None};
        if let Some(field)=field {bindings.insert(key.clone(),format!("{}.{}",db.id,field));}
      }
   }
   let mut dependencies=detected.dependencies.clone();
   for binding in bindings.values(){if let Some((service,field))=binding.split_once('.') {if !["url","host","port","username","password","database","bucket","endpoint","accessKey","secretKey"].contains(&field)||!project.services.iter().any(|s|s.id==service){plan.blockers.push(format!("Invalid environment binding: {binding}"));}else{dependencies.push(service.into());}}else{plan.blockers.push(format!("Invalid environment binding: {binding}"));}}
   dependencies.sort();dependencies.dedup();
   let version=choice.version.clone().or_else(||account.database_versions[&platform].as_str().map(str::to_string));
   if detected.kind=="database"&&existing.is_none()&&version.is_none(){plan.blockers.push(format!("{name}: select a supported database version."));}
   let mut resize=None;
   if detected.kind=="app"{
    let allocated=existing.as_ref().and_then(|id|resources.iter().find(|r|r["_id"].as_str()==Some(id)||r["project_id"].as_str()==Some(id))).and_then(|r|r["_launchDisks"].as_array());
    let required=if let Some(disks)=allocated{crate::storage::required_storage(&volumes,disks)?}else{disk_allocation(&volumes)};
    let actual_existing=existing.as_ref().and_then(|id|resources.iter().find(|r|r["_id"].as_str()==Some(id)||r["project_id"].as_str()==Some(id))).and_then(|r|r["planID"].as_str()).and_then(|id|available.iter().copied().find(|p|p.id==id&&previous_bundle.as_ref().or(choice.bundle_id.as_ref()).is_none_or(|bundle|p.bundle_id==*bundle)));
    if choices.guided&&existing.is_none()&&!choice.plan_locked&&selected.is_some_and(|p|p.disk<required){
     if let Some(larger)=available.iter().copied().filter(|p|p.disk>=required&&p.monthly_price.is_some()&&p.ram>=selected.map(|p|p.ram).unwrap_or(0.5)).min_by(|a,b|a.monthly_price.unwrap().total_cmp(&b.monthly_price.unwrap())){selected=Some(larger);plan.warnings.push(localized(language,"A plan with enough storage for all requested disks was selected. Review its cost before deployment.","برای مجموع دیسک‌ها پلنی با فضای کافی انتخاب شد. هزینه آن را پیش از استقرار بررسی کنید."));}
    }
    if let Some(current)=actual_existing{if required>current.disk{
     let explicit=choice.plan_locked;
     let target=if choices.guided&&!explicit{crate::storage::fitting_plan(&account.plans,current,&platform,required)}else{selected.filter(|p|p.disk>=required&&p.ram>=current.ram&&p.cpu>=current.cpu&&p.region==current.region&&p.bundle_id==current.bundle_id)};
     if let Some(target)=target{resize=Some(crate::storage::resize_advice(current,target,required)?);selected=Some(target);}else{plan.blockers.push(storage_capacity_message(&name,required,current.disk,language));}
    }}else if existing.is_none(){if let Some(capacity)=selected.map(|p|p.disk){if required>capacity{plan.blockers.push(storage_capacity_message(&name,required,capacity,language));}}}
   }
   let port=if files.iter().any(|f|f.path=="liara-start.py"){8080}else{choice.port.or_else(||rec.and_then(|r|r.port)).unwrap_or(detected.port)};
   if detected.kind=="app"&&port==0 {plan.blockers.push(format!("{name}: port must be between 1 and 65535."));}
   files.retain(|f|f.path!="liara.json");
   let disks:Vec<_>=volumes.iter().filter(|v|v.resolution.as_deref()==Some("disk")).map(|v|serde_json::json!({"name":v.disk_name,"mountTo":v.target})).collect();
   files.push(GeneratedFile{path:"liara.json".into(),content:serde_json::to_string_pretty(&serde_json::json!({"app":name,"platform":platform,"port":port,"disks":disks})).unwrap(),reason:if language=="fa"{"تنظیمات استقرار در نسخه موقت پروژه"}else{"Deployment settings in a staging copy"}.into()});
   let needs_source=detected.kind=="app"&&(image.is_none()||!volumes.is_empty());
   let source_hash=if needs_source{project::stage(&dir,&store.stage_dir(&plan.id,&detected.id),&files)?}else{String::new()};
   plan.services.push(ServicePlan{resize:resize.clone(),id:detected.id.clone(),name,kind:detected.kind.clone(),platform,root:detected.root.clone(),port,image,plan_id:selected.map(|p|p.id.clone()).unwrap_or_default(),bundle_id:selected.map(|p|p.bundle_id.clone()).unwrap_or_default(),monthly_price:if let Some(resize)=&resize{Some(resize.target_monthly_price)}else if existing.is_some(){Some(0.0)}else{selected.and_then(|p|p.monthly_price)},existing_id:existing,version,local_env_keys:project::local_env(&dir).keys().filter(|k|detected.env_keys.contains(k)).cloned().collect(),env_keys:detected.env_keys.clone(),bindings,dependencies,volumes,command:detected.command.clone(),files,source_hash,rationale:rec.map(|r|r.rationale.clone()).unwrap_or_else(||if language=="fa"{"پلن سازگار با کمترین هزینه برای شروع؛ قابل تغییر است."}else{"A modest compatible plan to start. You can choose another."}.into())});
   if let Some(service)=plan.services.last_mut().filter(|s|s.resize.is_some()){service.rationale=crate::storage::explain(service,language);}
 }
 if order(&plan.services).is_err(){plan.blockers.push("Service dependencies contain a cycle or an unknown service. Resolve the topology before deployment.".into());}
 if plan.services.iter().any(|s|s.monthly_price.is_none()){plan.blockers.push("Pricing could not be verified for every new service. Refresh the live catalog before approving.".into());}
 plan.blockers.sort();plan.blockers.dedup();plan.revision=revision(&plan);Ok(plan)
}
pub(crate) fn disk_allocation(volumes:&[VolumeMount])->f64{
 let mut disks=std::collections::BTreeMap::<&str,u32>::new();
 for volume in volumes.iter().filter(|v|v.resolution.as_deref()==Some("disk")){if let (Some(name),Some(size))=(volume.disk_name.as_deref(),volume.size_gb){disks.entry(name).and_modify(|n|*n=(*n).max(size)).or_insert(size);}}
 disks.values().map(|size|*size as f64).sum()
}
pub(crate) fn storage_capacity_message(name:&str,required:f64,capacity:f64,language:&str)->String{
 if language=="fa"{format!("not_enough_storage_space: برای دیسک‌های {name} در مجموع {required} گیگابایت لازم است، اما پلن برنامه {capacity} گیگابایت فضا دارد. دستیار پلن مناسب را برای تایید پیشنهاد می‌دهد. دیسک‌های موجود حذف یا کوچک نمی‌شوند.")}
 else{format!("not_enough_storage_space: {name} needs {required} GB across its disks, but the app plan provides {capacity} GB. The assistant can recommend a suitable plan for approval. Existing disks are never deleted or shrunk.")}
}
pub fn order(services:&[ServicePlan])->Result<Vec<usize>>{
 let mut done=BTreeSet::new();let mut result=vec![];
 while result.len()<services.len(){let next=services.iter().enumerate().find(|(_,s)|!done.contains(&s.id)&&s.dependencies.iter().all(|d|done.contains(d)));if let Some((i,s))=next{done.insert(s.id.clone());result.push(i);}else{return Err("Invalid service dependency graph.".into());}}
 Ok(result)
}
pub fn validate_secrets(plan:&DeploymentPlan,secrets:&Secrets)->Result<()> {
 for s in &plan.services {for key in &s.env_keys {if !s.bindings.contains_key(key)&&secrets.get(&s.id).and_then(|m|m.get(key)).is_none_or(|v|v.trim().is_empty()){return Err(format!("{}: enter a value for {key}.",s.name));}}}
 Ok(())
}
pub fn approval(plan:&DeploymentPlan)->Result<ApprovalRequest>{
 if !plan.blockers.is_empty(){return Err(plan.blockers.join("\n"));}
 if plan.revision!=revision(plan){return Err("Plan revision changed. Prepare it again.".into());}
 let mut actions=vec![];
 let fa=plan.language=="fa";
 if let Some(n)=plan.network.as_ref().and_then(|s|s.strip_prefix("new:")){actions.push(if fa{format!("ساخت شبکه خصوصی {n}")}else{format!("Create private network {n}")});}
 for s in &plan.services {
  let kind=if fa{match s.kind.as_str(){"database"=>"پایگاه داده","bucket"=>"فضای ذخیره‌سازی شیء","app"=>"برنامه",x=>x}}else{s.kind.as_str()};
  let action=if s.existing_id.is_some(){if fa{"استفاده از منبع موجود"}else{"Use existing"}}else if fa{"ساخت منبع"}else{"Create"};
  actions.push(if fa{format!("{action} {kind} {} از نوع {} با پلن {} / {}",s.name,s.platform,s.plan_id,s.bundle_id)}else{format!("{action} {} {} ({}, {} / {})",s.kind,s.name,s.platform,s.plan_id,s.bundle_id)});
  if s.files.iter().any(|f|f.path=="liara-start.py"){actions.push(if fa{"اجرای Nginx و برنامه وب در یک کانتینر با دیسک‌های مشترک؛ حذف نیاز به Nginx جداگانه و Certbot؛ TLS مدیریت‌شده لیارا پس از اتصال دامنه".into()}else{"Run Nginx and the web application in one container with shared disks; replace separate Nginx/Certbot deployment with Liara-managed TLS after domain setup".into()});}
  if s.resize.is_some(){actions.push(crate::storage::explain(s,&plan.language));}
  if s.kind=="app"{actions.push(if fa{format!("استقرار {} و تنظیم متغیرهای محیطی فهرست‌شده",s.name)}else{format!("Deploy {} and set its listed environment variables",s.name)});for v in s.volumes.iter().filter(|v|v.resolution.as_deref()==Some("disk")){actions.push(if fa{format!("ساخت دیسک پایدار لیارا {} با اندازه {} گیگابایت برای {} در مسیر {}",v.disk_name.as_deref().unwrap_or("disk"),v.size_gb.unwrap_or_default(),s.name,v.target)}else{format!("Create Liara persistent disk {} ({} GB) for {} at {}",v.disk_name.as_deref().unwrap_or("disk"),v.size_gb.unwrap_or_default(),s.name,v.target)});}}
 }
 Ok(ApprovalRequest{id:id(),plan_id:plan.id.clone(),revision:plan.revision.clone(),actions,monthly_price:plan.services.iter().try_fold(0.0,|total,s|s.monthly_price.map(|p|total+p)),expires_at:(chrono::Utc::now()+chrono::Duration::minutes(15)).to_rfc3339()})
}
#[cfg(test)]mod tests {use super::*;use crate::agent::ServiceAdvice;use serde_json::json;use std::{collections::BTreeMap,fs};
 #[test]fn tampered_plan_fails(){let mut p=DeploymentPlan{id:"a".into(),project_id:"b".into(),account_id:"c".into(),revision:String::new(),network:None,services:vec![],summary:"one".into(),warnings:vec![],blockers:vec![],created_at:now(),language:"en".into(),allow_config_repairs:false};p.revision=revision(&p);assert!(approval(&p).is_ok());p.summary="changed".into();assert!(approval(&p).is_err());}
 fn fixture()->(tempfile::TempDir,tempfile::TempDir,Store,ProjectAnalysis,Account,Advice){
  let dir=tempfile::tempdir().unwrap();fs::write(dir.path().join("Dockerfile"),"FROM python:3.12\nWORKDIR /app\nCMD [\"python\", \"app.py\"]\n").unwrap();fs::write(dir.path().join("app.py"),"print('ready')").unwrap();
  let app=DetectedService{id:"web".into(),name:"web".into(),kind:"app".into(),platform:"docker".into(),root:".".into(),port:8000,image:None,evidence:vec![],env_keys:vec!["RABBITMQ_HOST".into(),"RABBITMQ_PORT".into(),"RABBITMQ_USER".into(),"RABBITMQ_PASS".into(),"CELERY_BROKER_URL".into()],dependencies:vec!["broker".into()],warnings:vec![],volumes:vec![VolumeMount{id:"web-volume-0".into(),source:"uploads".into(),target:"/app/uploads".into(),kind:"volume".into(),read_only:false,..Default::default()}],build_command:None,start_command:None,command:None};
  let broker=DetectedService{id:"broker".into(),name:"broker".into(),kind:"database".into(),platform:"rabbitmq".into(),root:".".into(),port:5672,image:None,evidence:vec![],env_keys:vec![],dependencies:vec![],warnings:vec![],volumes:vec![],build_command:None,start_command:None,command:None};
  let project=ProjectAnalysis{id:"project".into(),path:dir.path().to_string_lossy().into(),name:"demo".into(),services:vec![app,broker],git_branch:None,git_dirty:false,file_count:2,warnings:vec![],analyzed_at:now()};
  let account=Account{id:"account".into(),name:"test".into(),email:String::new(),balance:None,credential_storage:"session".into(),plans:vec![PlanOption{id:"app-plan".into(),category:"app".into(),label:"App".into(),ram:1.0,cpu:1.0,disk:200.0,monthly_price:Some(100.0),bundle_id:"app-bundle".into(),region:String::new(),supports:vec![]},PlanOption{id:"db-plan".into(),category:"database".into(),label:"Database".into(),ram:1.0,cpu:1.0,disk:2.0,monthly_price:Some(100.0),bundle_id:"db-bundle".into(),region:String::new(),supports:vec!["rabbitmq".into()]}],networks:vec![],projects:vec![],databases:vec![],buckets:vec![],database_versions:json!({"rabbitmq":"3.13"}),warnings:vec![]};
  let advice=Advice{summary:"Review deployment".into(),services:vec![ServiceAdvice{service_id:"web".into(),rationale:"Web app".into(),files:vec![],port:None,platform:Some("rabbitmq".into()),plan_id:None,bundle_id:None,volumes:Default::default(),env_defaults:Default::default()}],warnings:vec![]};
  let store_dir=tempfile::tempdir().unwrap();let store=Store::new(store_dir.path()).unwrap();(dir,store_dir,store,project,account,advice)
 }
 #[test]fn guided_plans_fit_total_disks_and_existing_upgrades_require_explicit_approval(){
  let (_dir,_store_dir,store,project,mut account,mut advice)=fixture();account.plans[0].disk=5.0;
  let mut larger=account.plans[0].clone();larger.id="large-app".into();larger.disk=10.0;larger.monthly_price=Some(200.0);account.plans.push(larger);
  advice.services[0].plan_id=Some("app-plan".into());advice.services[0].volumes.insert("web-volume-0".into(),VolumeChoice{resolution:Some("disk".into()),disk_name:Some("media".into()),size_gb:Some(7)});
  let mut choices=PlanChoices{guided:true,..Default::default()};
  let plan=build(&store,&project,&account,&choices,&advice,"fa").unwrap();assert!(plan.blockers.is_empty(),"{:?}",plan.blockers);assert_eq!(plan.services[0].plan_id,"large-app");assert_eq!(plan.services[0].monthly_price,Some(200.0));
  choices.services.insert("web".into(),ServiceChoice{plan_id:Some("app-plan".into()),..Default::default()});let suggested=build(&store,&project,&account,&choices,&advice,"fa").unwrap();assert!(suggested.blockers.is_empty());assert_eq!(suggested.services[0].plan_id,"large-app");
  choices.services.insert("web".into(),ServiceChoice{plan_locked:true,plan_id:Some("app-plan".into()),..Default::default()});let manual=build(&store,&project,&account,&choices,&advice,"fa").unwrap();assert!(manual.blockers.iter().any(|b|b.contains("not_enough_storage_space")));assert_eq!(manual.services[0].plan_id,"app-plan");
  account.projects.push(json!({"_id":"app-id","project_id":"demo-web","type":"docker","planID":"app-plan"}));choices.services.insert("web".into(),ServiceChoice{existing_id:Some("app-id".into()),..Default::default()});let existing=build(&store,&project,&account,&choices,&advice,"fa").unwrap();assert!(existing.blockers.is_empty(),"{:?}",existing.blockers);assert_eq!(existing.services[0].existing_id.as_deref(),Some("app-id"));assert_eq!(existing.services[0].plan_id,"large-app");assert_eq!(existing.services[0].resize.as_ref().unwrap().from_plan_id,"app-plan");assert!(approval(&existing).unwrap().actions.iter().any(|a|a.contains("ارتقا")));
 }
 #[test]fn rabbitmq_is_managed_and_volume_choices_unblock_the_review(){
  let (_dir,_store_dir,store,project,account,advice)=fixture();
  let empty=PlanChoices::default();let unresolved=build(&store,&project,&account,&empty,&advice,"fa").unwrap();assert!(unresolved.blockers.iter().any(|b|b.contains("ذخیره‌سازی")));
  let choice=ServiceChoice{volumes:BTreeMap::from([("web-volume-0".into(),VolumeChoice{resolution:Some("disk".into()),disk_name:Some("demo-uploads".into()),size_gb:Some(10)})]),..Default::default()};let choices=PlanChoices{services:BTreeMap::from([("web".into(),choice)]),..Default::default()};
  let plan=build(&store,&project,&account,&choices,&advice,"fa").unwrap();assert!(plan.blockers.is_empty(),"{:?}",plan.blockers);
  let web=plan.services.iter().find(|s|s.id=="web").unwrap();assert_eq!(web.platform,"docker");assert_eq!(web.bindings["RABBITMQ_HOST"],"broker.host");assert_eq!(web.bindings["RABBITMQ_PORT"],"broker.port");assert_eq!(web.bindings["RABBITMQ_USER"],"broker.username");assert_eq!(web.bindings["RABBITMQ_PASS"],"broker.password");assert_eq!(web.bindings["CELERY_BROKER_URL"],"broker.url");
  let staged=fs::read_to_string(store.stage_dir(&plan.id,"web").join("liara.json")).unwrap();assert!(staged.contains("demo-uploads"));assert!(staged.contains("\"sizeGb\"")==false);
  let approval=approval(&plan).unwrap();assert!(approval.actions.iter().any(|a|a.contains("demo-uploads")&&a.contains("10 گیگابایت")));
 }
 #[test]fn guided_recommendations_are_validated_and_manual_choices_win(){
  let (_dir,_store_dir,store,project,account,mut advice)=fixture();
  let rec=&mut advice.services[0];rec.plan_id=Some("app-plan".into());rec.bundle_id=Some("app-bundle".into());rec.volumes.insert("web-volume-0".into(),VolumeChoice{resolution:Some("disk".into()),disk_name:Some("saved-files".into()),size_gb:Some(100)});
  let mut choices=PlanChoices{guided:true,answers:BTreeMap::from([("uploads".into(),"20 movies per month, 1 GB each, keep 3 months".into())]),..Default::default()};
  let plan=build(&store,&project,&account,&choices,&advice,"en").unwrap();assert!(plan.blockers.is_empty(),"{:?}",plan.blockers);assert_eq!(plan.services[0].volumes[0].size_gb,Some(100));assert_eq!(plan.services[0].plan_id,"app-plan");
  choices.services.insert("web".into(),ServiceChoice{plan_locked:true,plan_id:Some("app-plan".into()),bundle_id:Some("app-bundle".into()),volumes:BTreeMap::from([("web-volume-0".into(),VolumeChoice{resolution:Some("disk".into()),disk_name:Some("my-files".into()),size_gb:Some(10)})]),..Default::default()});
  let customized=build(&store,&project,&account,&choices,&advice,"en").unwrap();assert_eq!(customized.services[0].volumes[0].size_gb,Some(10));
  choices.services.clear();advice.services[0].plan_id=Some("invented-plan".into());let invalid=build(&store,&project,&account,&choices,&advice,"en").unwrap();assert!(!invalid.blockers.is_empty());
  advice.services[0].plan_id=Some("app-plan".into());advice.services[0].volumes.get_mut("web-volume-0").unwrap().size_gb=Some(2000);assert!(!build(&store,&project,&account,&choices,&advice,"en").unwrap().blockers.is_empty());
 }
 #[test]fn unresolved_saved_choices_no_longer_hide_agent_disk_recommendations(){
  let (_dir,_store_dir,store,project,account,mut advice)=fixture();
  advice.services[0].volumes.insert("web-volume-0".into(),VolumeChoice{resolution:Some("disk".into()),disk_name:Some("movie-files".into()),size_gb:Some(100)});
  let choices=PlanChoices{guided:true,services:BTreeMap::from([("web".into(),ServiceChoice{volumes:BTreeMap::from([("web-volume-0".into(),VolumeChoice::default())]),..Default::default()})]),..Default::default()};
  let plan=build(&store,&project,&account,&choices,&advice,"en").unwrap();assert!(plan.blockers.is_empty(),"{:?}",plan.blockers);assert_eq!(plan.services[0].volumes[0].size_gb,Some(100));
  let no_advice=Advice{services:vec![],..advice};let fallback=build(&store,&project,&account,&choices,&no_advice,"en").unwrap();assert!(fallback.blockers.is_empty(),"{:?}",fallback.blockers);assert_eq!(fallback.services[0].volumes[0].size_gb,Some(10));
 }
 #[test]fn guided_mode_does_not_copy_named_runtime_volumes_into_an_image(){
  let (_dir,_store_dir,store,project,account,mut advice)=fixture();advice.services[0].volumes.insert("web-volume-0".into(),VolumeChoice{resolution:Some("copy".into()),disk_name:None,size_gb:None});let choices=PlanChoices{guided:true,..Default::default()};let plan=build(&store,&project,&account,&choices,&advice,"en").unwrap();assert!(plan.blockers.is_empty(),"{:?}",plan.blockers);assert_eq!(plan.services[0].volumes[0].resolution.as_deref(),Some("disk"));
 }
 #[test]fn guided_mode_blocks_shared_disks_instead_of_promising_file_sharing(){
  let (_dir,_store_dir,store,mut project,account,mut advice)=fixture();
  let mut second=project.services[0].clone();second.id="worker".into();second.name="worker".into();second.volumes[0].id="worker-files".into();project.services.push(second);
  advice.services[0].volumes.insert("web-volume-0".into(),VolumeChoice{resolution:Some("disk".into()),disk_name:Some("saved-files".into()),size_gb:Some(10)});
  let choices=PlanChoices{guided:true,..Default::default()};let plan=build(&store,&project,&account,&choices,&advice,"en").unwrap();assert!(plan.blockers.iter().any(|b|b.contains("shares saved files")));
 }
 #[test]fn readonly_image_mounts_are_copied_into_a_reviewable_staged_build(){
  let (_dir,_store_dir,store,mut project,account,advice)=fixture();project.services.retain(|s|s.id=="web");let app=&mut project.services[0];app.image=Some("nginx:1.27-alpine".into());app.dependencies.clear();app.env_keys.clear();app.volumes=vec![VolumeMount{id:"web-volume-0".into(),source:"nginx.conf".into(),target:"/etc/nginx/conf.d/default.conf".into(),kind:"bind".into(),read_only:true,..Default::default()}];app.command=Some(json!(["nginx","-g","daemon off;"]));fs::write(std::path::Path::new(&project.path).join("nginx.conf"),"server { listen 80; }").unwrap();
  let plan=build(&store,&project,&account,&PlanChoices::default(),&advice,"en").unwrap();assert!(plan.blockers.is_empty(),"{:?}",plan.blockers);let service=plan.services.iter().find(|s|s.id=="web").unwrap();assert!(service.image.is_none());let dockerfile=fs::read_to_string(store.stage_dir(&plan.id,"web").join("Dockerfile")).unwrap();assert!(dockerfile.contains("FROM nginx:1.27-alpine"));assert!(dockerfile.contains("COPY [\"nginx.conf\",\"/etc/nginx/conf.d/default.conf\"]"));assert!(dockerfile.contains("CMD [\"nginx\",\"-g\",\"daemon off;\"]"));
 }
}
