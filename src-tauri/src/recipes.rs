//! Bounded staging recipes for deployment topologies that cannot use separate disks.
use crate::{models::*,security::*,project};
use std::{collections::BTreeMap,path::Path};
use regex::Regex;

pub struct Recipe {pub service_id:String,pub files:Vec<GeneratedFile>}
fn read(root:&Path,path:&str)->Result<String>{if sensitive_path(Path::new(path)){return Err("Credential files cannot be used as deployment configuration.".into());}let file=scoped(root,path)?;if std::fs::metadata(&file).map_err(|e|e.to_string())?.len()>150_000{return Err("Deployment configuration is too large for the automatic recipe.".into());}std::fs::read_to_string(file).map_err(|e|e.to_string())}
fn remove_block(mut text:String,start:&str)->Result<String>{
 let pattern=Regex::new(&format!(r"(?m)^[ \t]*{}\s*\{{",regex::escape(start))).map_err(|e|e.to_string())?;
 while let Some(found)=pattern.find(&text){let begin=found.start();
  let open=begin+text[begin..].find('{').ok_or("Unclosed Nginx block")?;
  let mut depth=0;let mut quote=None;let mut comment=false;let mut escape=false;let mut end=None;
  for (offset,c) in text[open..].char_indices(){
   if comment{if c=='\n'{comment=false;}continue;}
   if escape{escape=false;continue;}if c=='\\'{escape=true;continue;}
   if let Some(q)=quote{if c==q{quote=None;}continue;}
   if c=='#'{comment=true;continue;}if c=='\''||c=='"'{quote=Some(c);continue;}
   if c=='{'{depth+=1;}if c=='}'{depth-=1;if depth==0{end=Some(open+offset+1);break;}}
  }
  text.replace_range(begin..end.ok_or("Unclosed Nginx block")?,"");
 }
 Ok(text)
}
fn adapt_nginx(text:&str,web:&str,port:u16,paths:&BTreeMap<String,String>)->Result<String>{
 let mut text=remove_block(text.into(),"location /.well-known/acme-challenge/")?;
 text=remove_block(text,"if ($redirect_to_https)")?;
 let listen=Regex::new(r"^listen\s+([^;]+);").unwrap();
 let upstream=Regex::new(&format!(r"\bserver\s+{}:{}\s*;",regex::escape(web),port)).unwrap();
 let mut out=String::new();
 for line in text.lines(){
  let code=line.split('#').next().unwrap_or("").trim();
  if code.starts_with("ssl_")||code.starts_with("http2 ")||code.starts_with("include ")&&code.contains("/tls.conf"){continue;}
  if let Some(c)=listen.captures(code){
   let address=c[1].split_whitespace().next().unwrap_or("");
   match address{"80"=>out.push_str("    listen 8080;\n"),"443"|"[::]:80"|"[::]:443"=>{},_=>return Err("Automatic Nginx grouping only supports standard HTTP/HTTPS listeners.".into())}
   continue;
  }
  let mut line=upstream.replace_all(line,"server 127.0.0.1:8000;").to_string();
  line=line.replace("/etc/nginx/snippets/","/app/liara-proxy/snippets/");
  for (from,to) in paths.iter().rev(){line=line.replace(from,to);}
  out.push_str(&line);out.push('\n');
 }
 if out.lines().map(|l|l.split('#').next().unwrap_or("")).any(|l|l.contains("/etc/letsencrypt")||l.contains("/var/www/certbot")){return Err("Nginx contains an unsupported certificate dependency; automatic grouping was not applied.".into());}
 Ok(out)
}

pub fn reuse_associated_resources(store:&crate::store::Store,project:&ProjectAnalysis,account:&Account,choices:&mut PlanChoices)->Result<()>{
 let runs:Vec<DeploymentRun>=store.list("run",&account.id)?;
 for service in &project.services{
  if choices.services.get(&service.id).is_some_and(|c|c.existing_id.is_some()){continue;}
  for run in &runs{
   if run.plan.account_id!=account.id{continue;}
   let Ok(previous)=store.get::<ProjectAnalysis>("project",&run.plan.project_id)else{continue};if previous.path!=project.path{continue;}
   let Some(progress)=run.resources.iter().find(|r|r.service_id==service.id&&r.resource_id.is_some())else{continue};
   let Some(old)=run.plan.services.iter().find(|s|s.id==service.id&&s.platform==service.platform)else{continue};
   if choices.services.get(&service.id).and_then(|c|c.name.as_ref()).is_some_and(|name|name!=&old.name){continue;}
   let resources=match service.kind.as_str(){"database"=>&account.databases,"bucket"=>&account.buckets,_=>&account.projects};
   let id=progress.resource_id.as_deref().unwrap();
   if !resources.iter().any(|r|["_id","DBId","id","project_id"].iter().any(|key|r[key].as_str()==Some(id))&&["project_id","hostname","name"].iter().any(|key|r[key].as_str()==Some(old.name.as_str()))&&r["type"].as_str().is_none_or(|kind|kind==service.platform)){continue;}
   let choice=choices.services.entry(service.id.clone()).or_default();choice.existing_id=Some(id.into());choice.name=Some(old.name.clone());break;
  }
 }
 Ok(())
}

pub fn deployment_hosts(service:&ServicePlan,values:&mut BTreeMap<String,String>){
 if !service.files.iter().any(|f|f.path=="liara-start.py"){return;}
 let mut hosts=vec![format!("{}.liara.run",service.name)];
 if let Some(config)=service.files.iter().find(|f|f.path=="liara-proxy/default.conf"){
  let names=Regex::new(r"(?m)^\s*server_name\s+([^;]+);").unwrap();let domain=Regex::new(r"^[a-zA-Z0-9.-]+$").unwrap();
  for capture in names.captures_iter(&config.content){for name in capture[1].split_whitespace(){if name.contains('.')&&domain.is_match(name)&&!hosts.iter().any(|h|h==name){hosts.push(name.into());}}}
 }
 for (key,origins) in [("ALLOWED_HOSTS",false),("CSRF_TRUSTED_ORIGINS",true)]{
  let entry=values.entry(key.into()).or_default();let mut items:Vec<String>=entry.split(',').map(str::trim).filter(|s|!s.is_empty()).map(str::to_string).collect();for host in &hosts{let value=if origins{format!("https://{host}")}else{host.clone()};if !items.contains(&value){items.push(value);}}*entry=items.join(",");
 }
}

/// Recognizes a Debian Python/Django image and its local Nginx front end.
/// Processes share one filesystem; managed databases remain separate resources.
pub fn grouped_django_nginx(project:&mut ProjectAnalysis,language:&str)->Result<Option<Recipe>>{
 let proxies:Vec<_>=project.services.iter().filter(|s|s.image.as_deref().is_some_and(|i|i.split(':').next()==Some("nginx"))).collect();
 if proxies.len()!=1{return Ok(None);}let proxy=proxies[0].clone();
 let apps:Vec<_>=project.services.iter().filter(|s|s.kind=="app"&&s.id!=proxy.id).collect();
 if apps.len()!=1{return Ok(None);}let app=apps[0].clone();let root=Path::new(&project.path);
 if app.root!="."||!root.join("manage.py").is_file(){return Ok(None);}
 let docker=read(root,"Dockerfile")?;
 if docker.contains("<<")||docker.lines().any(|l|{let line=l.trim();line.starts_with("USER ")&&!matches!(line,"USER root"|"USER 0")}){return Ok(None);}
 let from=Regex::new(r"(?m)^FROM\s+python:[^\s]+(?:\s+AS\s+\S+)?\s*$").unwrap();
 if !from.is_match(&docker)||docker.contains("alpine")||Regex::new(r"(?im)^FROM\s+").unwrap().find_iter(&docker).count()!=1||!Regex::new(r"(?m)^WORKDIR\s+/app\s*$").unwrap().is_match(&docker)||!Regex::new(r"(?m)^COPY\s+\.\s+\.\s*$").unwrap().is_match(&docker){return Ok(None);}
 let cmd=if let Some(command)=&app.command{if let Some(s)=command.as_str(){vec!["/bin/sh".into(),"-c".into(),s.into()]}else{serde_json::from_value::<Vec<String>>(command.clone()).map_err(|_|"Unsupported application command")?}}else{
  let re=Regex::new(r"(?m)^CMD\s+(\[.+\])\s*$").unwrap();let Some(c)=re.captures(&docker)else{return Ok(None)};serde_json::from_str::<Vec<String>>(&c[1]).map_err(|_|"Unsupported application command")?
 };
 // Keep the proven application listener local and place Nginx on the public port.
 if app.port!=8000||!cmd.iter().any(|s|s.contains(":8000")){return Ok(None);}
 let entry_re=Regex::new(r"(?m)^ENTRYPOINT\s+(\[.+\])\s*$").unwrap();
 if docker.lines().any(|l|l.trim_start().starts_with("ENTRYPOINT "))&&!entry_re.is_match(&docker){return Ok(None);}
 if let Some(c)=Regex::new(r"(?m)^ENTRYPOINT\s+(\[.+\])\s*$").unwrap().captures(&docker){
  let entry:Vec<String>=serde_json::from_str(&c[1]).map_err(|_|"Unsupported entrypoint")?;
  if entry.len()!=1{return Ok(None);}let pattern=format!(r"(?m)^COPY\s+(\S+)\s+{}\s*$",regex::escape(&entry[0]));
  let re=Regex::new(&pattern).map_err(|e|e.to_string())?;let Some(copy)=re.captures(&docker)else{return Ok(None)};
  if !read(root,&copy[1])?.contains("exec \"$@\""){return Ok(None);}
 }
 let Some(main)=proxy.volumes.iter().find(|v|v.target=="/etc/nginx/conf.d/default.conf"&&v.kind=="bind"&&v.read_only)else{return Ok(None)};
 let mut paths=BTreeMap::new();let mut snippets=Vec::new();
 for volume in &proxy.volumes{
  if volume.id==main.id||["/etc/letsencrypt","/var/www/certbot"].contains(&volume.target.as_str()){continue;}
  if let Some(shared)=app.volumes.iter().find(|v|v.source.trim_start_matches("./")==volume.source.trim_start_matches("./")&&(v.kind=="bind")== (volume.kind=="bind")){
   paths.insert(volume.target.clone(),shared.target.clone());continue;
  }
  if volume.kind!="bind"||!volume.read_only{return Ok(None);}
  let source=scoped(root,&volume.source)?;if !source.exists(){return Ok(None);}
  if volume.target=="/etc/nginx/snippets"{
   for file in project::files(&source)?{let relative=file.strip_prefix(&source).map_err(|e|e.to_string())?.to_string_lossy().to_string();
    if relative.ends_with(".conf")&&relative!="tls.conf"{snippets.push((relative,read(&source,&file.strip_prefix(&source).unwrap().to_string_lossy())?));}
   }
  }else{
   // The application's COPY . . already includes immutable project assets.
   let rel=source.strip_prefix(root).map_err(|_|"Static asset path escapes the selected project")?;
   let uploaded=project::files(root)?;if !uploaded.iter().any(|p|p.starts_with(&source)){return Ok(None);}
   paths.insert(volume.target.clone(),format!("/app/{}",rel.to_string_lossy()));
  }
 }
 let fa=language=="fa";let reason=if fa{"اجرای Django و Nginx در یک برنامه با فایل‌های مشترک؛ TLS توسط لیارا"}else{"Run Django and Nginx in one application with shared files; TLS is handled by Liara"};
 let config=adapt_nginx(&read(root,&main.source)?,&app.name,app.port,&paths)?;
 if !config.contains("listen 8080;")||!config.contains("server 127.0.0.1:8000;"){return Ok(None);}
 let mut generated=vec![GeneratedFile{path:"liara-proxy/default.conf".into(),content:config,reason:reason.into()}];
 for (name,content) in snippets{generated.push(GeneratedFile{path:format!("liara-proxy/snippets/{name}"),content:adapt_nginx(&content,&app.name,app.port,&paths)?,reason:reason.into()});}
 generated.push(GeneratedFile{path:"liara-proxy/nginx.conf".into(),content:"worker_processes auto;\npid /tmp/liara-nginx.pid;\nerror_log /dev/stderr;\nevents { worker_connections 1024; }\nhttp {\n client_body_temp_path /tmp/liara-client-body;\n proxy_temp_path /tmp/liara-proxy-temp;\n include /etc/nginx/mime.types;\n access_log /dev/stdout;\n include /app/liara-proxy/default.conf;\n}\n".into(),reason:reason.into()});
 let runner=include_str!("grouped_runner.py").replace("__APP_COMMAND__",&serde_json::to_string(&serde_json::to_string(&cmd).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?);
 generated.push(GeneratedFile{path:"liara-start.py".into(),content:runner,reason:reason.into()});
 let mut docker=docker.lines().filter(|l|!l.trim_start().starts_with("CMD ")).collect::<Vec<_>>().join("\n");
 docker.push_str("\nUSER root\nRUN apt-get update && apt-get install -y --no-install-recommends nginx && rm -rf /var/lib/apt/lists/*\nEXPOSE 8080\nCMD [\"python\", \"/app/liara-start.py\"]\n");
 generated.push(GeneratedFile{path:"Dockerfile".into(),content:docker,reason:reason.into()});
 project.services.retain(|s|s.id!=proxy.id);
 for service in &mut project.services{service.dependencies.retain(|d|d!=&proxy.id);if service.id==app.id{service.port=8080;service.command=None;for key in ["ALLOWED_HOSTS","CSRF_TRUSTED_ORIGINS"]{if !service.env_keys.iter().any(|k|k==key){service.env_keys.push(key.into());}}service.evidence.push(reason.into());}}
 project.warnings.push(if fa{"Nginx با برنامه وب یکپارچه شد؛ فایل‌های رسانه و استاتیک روی همان دیسک در دسترس هر دو هستند. برنامه جداگانه Nginx ساخته نمی‌شود. منابع قبلی لیارا خودکار حذف نمی‌شوند. دامنه‌های سفارشی و TLS را پس از استقرار از بخش دامنه متصل کنید."}else{"Nginx is grouped with the web app so media and static files share its disks. No separate Nginx app is created. Previously created Liara resources are not deleted. Connect custom domains and managed TLS after deployment."}.into());
 Ok(Some(Recipe{service_id:app.id,files:generated}))
}

#[cfg(test)]mod tests{
 use super::*;use crate::{agent::{Advice,ServiceAdvice},planner,store::Store};use serde_json::json;use std::fs;
 fn fixture()->tempfile::TempDir{
  let dir=tempfile::tempdir().unwrap();fs::create_dir_all(dir.path().join("nginx/snippets")).unwrap();fs::create_dir_all(dir.path().join("partners-site")).unwrap();
  fs::write(dir.path().join("manage.py"),"# Django entrypoint\n").unwrap();fs::write(dir.path().join("requirements.txt"),"Django\ngunicorn\n").unwrap();fs::write(dir.path().join("partners-site/index.html"),"static website").unwrap();
  fs::write(dir.path().join("Dockerfile"),"FROM python:3.12-slim\nWORKDIR /app\nCOPY . .\nEXPOSE 8000\nCMD [\"gunicorn\",\"config.wsgi:application\",\"--bind\",\"0.0.0.0:8000\"]\n").unwrap();
  fs::write(dir.path().join("docker-compose.yml"),r#"services:
  db:
    image: postgres:16-alpine
  web:
    build: .
    depends_on: [db]
    volumes:
      - ./media:/app/media
      - staticfiles:/app/staticfiles
  nginx:
    image: nginx:1.27-alpine
    depends_on: [web]
    ports: ['80:80']
    volumes:
      - ./nginx/default.conf:/etc/nginx/conf.d/default.conf:ro
      - ./nginx/snippets:/etc/nginx/snippets:ro
      - ./media:/var/www/media:ro
      - staticfiles:/var/www/static:ro
      - ./partners-site:/var/www/partners-site:ro
      - certbot_conf:/etc/letsencrypt:ro
      - certbot_www:/var/www/certbot:ro
volumes:
  staticfiles:
  certbot_conf:
  certbot_www:
"#).unwrap();
  fs::write(dir.path().join("nginx/default.conf"),"upstream django { server web:8000; }\nserver {\n server_name shop.example www.shop.example;\n listen 80;\n listen 443 ssl;\n ssl_certificate /etc/letsencrypt/live/site/fullchain.pem;\n include /etc/nginx/snippets/tls.conf;\n location /assets/ { alias /var/www/partners-site/; }\n include /etc/nginx/snippets/locations.conf;\n}\n").unwrap();
  fs::write(dir.path().join("nginx/snippets/locations.conf"),"location /.well-known/acme-challenge/ { root /var/www/certbot; }\nlocation /media/ { alias /var/www/media/; }\nlocation /static/ { alias /var/www/static/; }\nlocation / {\n if ($redirect_to_https) { return 301 https://$host$request_uri; }\n proxy_pass http://django;\n}\n").unwrap();dir
 }
 fn build(project:&ProjectAnalysis,recipe:Recipe)->DeploymentPlan{
  let account:Account=serde_json::from_value(json!({"id":"test","name":"Test","email":"","balance":null,"credentialStorage":"session","plans":[{"id":"app","category":"app","label":"App","ram":1,"cpu":1,"disk":20,"monthlyPrice":10,"bundleId":"basic","region":"","supports":["docker"]},{"id":"db","category":"database","label":"DB","ram":1,"cpu":1,"disk":5,"monthlyPrice":10,"bundleId":"basic","region":"","supports":["postgres"]}],"networks":[],"projects":[],"databases":[],"buckets":[],"databaseVersions":{"postgres":"16"},"warnings":[]})).unwrap();
  let advice=Advice{summary:"One web application and managed PostgreSQL".into(),services:vec![ServiceAdvice{service_id:recipe.service_id,rationale:"Shared filesystem in one application".into(),files:recipe.files,port:Some(8080),platform:Some("docker".into()),plan_id:None,bundle_id:None,volumes:BTreeMap::new(),env_defaults:BTreeMap::new()}],warnings:vec![]};
  let data=tempfile::tempdir().unwrap();let store=Store::new(data.path()).unwrap();planner::build(&store,project,&account,&PlanChoices{guided:true,..Default::default()},&advice,"en").unwrap()
 }
 #[test]fn shared_media_and_static_are_grouped_without_changing_source(){
  let dir=fixture();let before=read(dir.path(),"Dockerfile").unwrap();let mut project=project::analyze(dir.path().to_str().unwrap(),"en").unwrap();
  let recipe=grouped_django_nginx(&mut project,"en").unwrap().unwrap();assert_eq!(project.services.len(),2);assert_eq!(project.services.iter().find(|s|s.kind=="database").unwrap().port,5432);
  let config=&recipe.files.iter().find(|f|f.path=="liara-proxy/snippets/locations.conf").unwrap().content;assert!(config.contains("/app/media/"));assert!(config.contains("/app/staticfiles/"));assert!(!config.contains("certbot"));assert!(!config.contains("return 301"));
  let main=&recipe.files.iter().find(|f|f.path=="liara-proxy/default.conf").unwrap().content;assert!(main.contains("127.0.0.1:8000"));assert!(main.contains("listen 8080;"));assert!(!main.contains("ssl_certificate"));assert!(main.contains("/app/partners-site/"));
  let plan=build(&project,recipe);assert!(plan.blockers.is_empty(),"{:?}",plan.blockers);assert_eq!(plan.services.len(),2);let web=plan.services.iter().find(|s|s.kind=="app").unwrap();let mut env=BTreeMap::from([("ALLOWED_HOSTS".into(),"my-domain.example".into())]);deployment_hosts(web,&mut env);assert!(env["ALLOWED_HOSTS"].contains("my-domain.example"));assert!(env["ALLOWED_HOSTS"].contains("shop.example"));assert!(env["CSRF_TRUSTED_ORIGINS"].contains("https://www.shop.example"));assert!(env["ALLOWED_HOSTS"].contains(&format!("{}.liara.run",web.name)));assert!(planner::approval(&plan).unwrap().actions.iter().any(|a|a.contains("one container")));assert_eq!(read(dir.path(),"Dockerfile").unwrap(),before);
 }
 #[test]fn grouping_reuses_only_recorded_live_resources_from_the_same_folder(){
  let dir=fixture();let mut project=project::analyze(dir.path().to_str().unwrap(),"en").unwrap();let recipe=grouped_django_nginx(&mut project,"en").unwrap().unwrap();let plan=build(&project,recipe);let web=plan.services.iter().find(|s|s.kind=="app").unwrap();let db=plan.services.iter().find(|s|s.kind=="database").unwrap();
  let mut account=Account{id:"owner".into(),name:String::new(),email:String::new(),balance:None,credential_storage:"session".into(),plans:vec![],networks:vec![],projects:vec![json!({"_id":"web-id","project_id":web.name,"type":"docker"})],databases:vec![json!({"DBId":"db-id","hostname":db.name,"type":"postgres"})],buckets:vec![],database_versions:json!({}),warnings:vec![]};
  let run=DeploymentRun{id:"run".into(),plan:DeploymentPlan{account_id:account.id.clone(),..plan.clone()},status:"failed".into(),started_at:now(),updated_at:now(),network_id:None,network_pending:false,resources:vec![ResourceProgress{service_id:web.id.clone(),resource_id:Some("web-id".into()),..Default::default()},ResourceProgress{service_id:db.id.clone(),resource_id:Some("db-id".into()),..Default::default()}],events:vec![]};
  let data=tempfile::tempdir().unwrap();let store=Store::new(data.path()).unwrap();store.save("project",&project.id,&account.id,&project).unwrap();store.save("run",&run.id,&account.id,&run).unwrap();
  let mut choices=PlanChoices::default();reuse_associated_resources(&store,&project,&account,&mut choices).unwrap();assert_eq!(choices.services[&web.id].existing_id.as_deref(),Some("web-id"));assert_eq!(choices.services[&db.id].existing_id.as_deref(),Some("db-id"));
  choices.services.get_mut(&web.id).unwrap().existing_id=None;choices.services.get_mut(&web.id).unwrap().name=Some("a-different-new-app".into());reuse_associated_resources(&store,&project,&account,&mut choices).unwrap();assert!(choices.services[&web.id].existing_id.is_none());
  account.projects.clear();choices.services.clear();reuse_associated_resources(&store,&project,&account,&mut choices).unwrap();assert!(!choices.services.contains_key(&web.id));
  account.id="different-team".into();choices.services.clear();reuse_associated_resources(&store,&project,&account,&mut choices).unwrap();assert!(choices.services.is_empty());
 }
 #[test]fn unsupported_base_image_is_not_rewritten(){let dir=fixture();fs::write(dir.path().join("Dockerfile"),"FROM python:3.12-alpine\nWORKDIR /app\nCOPY . .\nEXPOSE 8000\nCMD [\"gunicorn\",\"app\",\"--bind\",\"0.0.0.0:8000\"]").unwrap();let mut project=project::analyze(dir.path().to_str().unwrap(),"en").unwrap();assert!(grouped_django_nginx(&mut project,"en").unwrap().is_none());}
 #[test]fn quoted_application_commands_remain_data_in_the_runner(){
  let dir=fixture();let mut project=project::analyze(dir.path().to_str().unwrap(),"en").unwrap();project.services.iter_mut().find(|s|s.id=="web").unwrap().command=Some(json!(["python","-c","print(\"''' :8000\")"]));let recipe=grouped_django_nginx(&mut project,"en").unwrap().unwrap();let runner=recipe.files.iter().find(|f|f.path=="liara-start.py").unwrap();let file=dir.path().join("runner.py");fs::write(&file,&runner.content).unwrap();assert!(std::process::Command::new("python3").arg("-m").arg("py_compile").arg(file).status().unwrap().success());
 }
 #[test]#[ignore="Local read-only integration check; set LIARA_RECIPE_PROJECT explicitly"]fn real_project_recipe_check(){
  let path=std::env::var("LIARA_RECIPE_PROJECT").expect("Set LIARA_RECIPE_PROJECT");let root=Path::new(&path);let before=read(root,"Dockerfile").unwrap();let mut project=project::analyze(&path,"fa").unwrap();let recipe=grouped_django_nginx(&mut project,"fa").unwrap().expect("A supported grouped recipe");
  if let Ok(output)=std::env::var("LIARA_RECIPE_EXPORT"){for f in &recipe.files{let path=Path::new(&output).join(&f.path);fs::create_dir_all(path.parent().unwrap()).unwrap();fs::write(path,&f.content).unwrap();}}
  let plan=build(&project,recipe);assert!(plan.blockers.is_empty(),"{:?}",plan.blockers);assert_eq!(plan.services.iter().filter(|s|s.kind=="app").count(),1);assert_eq!(read(root,"Dockerfile").unwrap(),before);
 }
}
