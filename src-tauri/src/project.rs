use crate::{models::*, security::*};
use serde_json::{json,Value};
use std::{collections::{BTreeMap,BTreeSet}, fs, path::{Path,PathBuf},process::Command};
use ignore::WalkBuilder;

pub fn files(root:&Path) -> Result<Vec<PathBuf>> {
    let mut builder=WalkBuilder::new(root);
    builder.hidden(false).follow_links(false).require_git(false).git_ignore(true).git_global(false).git_exclude(false);
    builder.add_custom_ignore_filename(".dockerignore");
    builder.add_custom_ignore_filename(".liaraignore");
    let base=root.to_path_buf();
    builder.filter_entry(move |e| e.path()==base || !sensitive_path(e.path().strip_prefix(&base).unwrap_or(e.path())));
    let mut result=Vec::new();
    for e in builder.build() {
        let e=e.map_err(|e|e.to_string())?;
        if e.file_type().is_some_and(|f|f.is_file()) && !sensitive_path(e.path().strip_prefix(root).unwrap_or(e.path())) {result.push(e.into_path());}
        if result.len()>50_000 {return Err("Project contains over 50,000 deployable files. Add a .liaraignore file.".into());}
    }
    result.sort(); Ok(result)
}
pub fn source_hash(root:&Path)->Result<String> {
    use sha2::{Digest,Sha256};let mut h=Sha256::new();let mut total=0u64;
    for p in files(root)? { let bytes=fs::read(&p).map_err(|e|e.to_string())?;total+=bytes.len() as u64;
        if total>1024*1024*1024 {return Err("Uncompressed project exceeds 1 GiB. Exclude unnecessary files.".into());}
        h.update(p.strip_prefix(root).unwrap().to_string_lossy().as_bytes());h.update([0]);h.update((bytes.len() as u64).to_le_bytes());h.update(bytes);
    } Ok(format!("{:x}",h.finalize()))
}
fn read(path:&Path)->String {fs::read_to_string(path).unwrap_or_default()}
fn json_file(path:&Path)->Value {serde_json::from_str(&read(path)).unwrap_or(json!({}))}
fn env_keys(root:&Path)->Vec<String> {
    let mut keys=BTreeSet::new();
    let assign=regex::Regex::new(r"(?m)^\s*(?:export\s+)?([A-Z][A-Z0-9_]+)\s*=").unwrap();
    for name in [".env.example",".env.sample",".env.template",".env"] {
        // Only key names are retained. Values never leave this function.
        if let Ok(path)=scoped(root,name) {for c in assign.captures_iter(&read(&path)){keys.insert(c[1].into());}}
    }
    let re=regex::Regex::new(r#"(?:process\.env\.|import\.meta\.env\.)([A-Z][A-Z0-9_]+)|(?:getenv|environ\.get|env)\(["']([A-Z][A-Z0-9_]+)["']|environ\[["']([A-Z][A-Z0-9_]+)["']\]"#).unwrap();
    for path in files(root).unwrap_or_default().iter().filter(|p|matches!(p.extension().and_then(|x|x.to_str()),Some("ts"|"tsx"|"js"|"jsx"|"py"|"php"))).take(400) {
        if fs::metadata(path).map(|m|m.len()>150_000).unwrap_or(true){continue;}
        for c in re.captures_iter(&read(path)){if let Some(k)=c.get(1).or(c.get(2)).or(c.get(3)){keys.insert(k.as_str().into());}}
    }
    for k in ["NODE_ENV","PORT","HOST","CI"] {keys.remove(k);}
    keys.into_iter().collect()
}
fn detect(root:&Path,relative:&str,name:&str)->Option<DetectedService> {
    let package=json_file(&root.join("package.json"));
    let deps=format!("{} {}",package["dependencies"],package["devDependencies"]);
    let requirements=format!("{} {}",read(&root.join("requirements.txt")),read(&root.join("pyproject.toml"))).to_lowercase();
    let composer=read(&root.join("composer.json"));
    let mut evidence=Vec::new();let mut warnings=Vec::new();
    let platform=if root.join("Dockerfile").exists(){evidence.push("Dockerfile".into());"docker"}
      else if root.join("package.json").exists(){evidence.push("package.json".into());
        if deps.contains("\"next\""){"next"}else if deps.contains("\"@angular/core\""){"angular"}else if deps.contains("\"vue\""){"vue"}else if deps.contains("\"react\"") && !deps.contains("\"express\""){"react"}else{"node"}}
      else if root.join("manage.py").exists(){evidence.push("manage.py".into());"django"}
      else if requirements.contains("flask"){evidence.push("Flask dependency".into());"flask"}
      else if requirements.contains("fastapi"){evidence.push("FastAPI dependency".into());"python"}
      else if !requirements.trim().is_empty(){evidence.push("Python dependency manifest".into());"python"}
      else if composer.contains("laravel/framework"){evidence.push("Laravel composer.json".into());"laravel"}
      else if root.join("composer.json").exists() || root.join("index.php").exists(){evidence.push("PHP files".into());"php"}
      else if root.join("go.mod").exists(){evidence.push("go.mod".into());"go"}
      else if fs::read_dir(root).ok()?.filter_map(|x|x.ok()).any(|e|e.path().extension().is_some_and(|x|x=="csproj")){evidence.push(".csproj".into());"dotnet"}
      else if root.join("Gemfile").exists(){evidence.push("Gemfile (Docker recipe needed)".into());warnings.push("Ruby requires a Dockerfile; the agent will propose one for review.".into());"docker"}
      else if root.join("index.html").exists(){evidence.push("index.html".into());"static"}else{return None};
    let mut port=if matches!(platform,"static"|"php"|"laravel"|"react"|"vue"|"angular"){80}else if matches!(platform,"python"|"django"|"flask"){8000}else{3000};
    let docker=read(&root.join("Dockerfile"));
    if let Some(c)=regex::Regex::new(r"(?im)^EXPOSE\s+(\d+)").unwrap().captures(&docker){port=c[1].parse().unwrap_or(port);}
    let config=json_file(&root.join("liara.json"));if let Some(n)=config["port"].as_u64().filter(|n|*n>0&&*n<=65535){port=n as u16;}
    if root.join("pnpm-lock.yaml").exists(){evidence.push("pnpm-lock.yaml".into());}else if root.join("yarn.lock").exists(){evidence.push("yarn.lock".into());}else if root.join("package-lock.json").exists(){evidence.push("package-lock.json".into());}
    Some(DetectedService{id:slug(name),name:name.into(),kind:"app".into(),platform:platform.into(),root:relative.into(),port,image:config["image"].as_str().map(str::to_string),evidence,env_keys:env_keys(root),dependencies:vec![],warnings,volumes:vec![],build_command:package["scripts"]["build"].as_str().map(str::to_string),start_command:package["scripts"]["start"].as_str().map(str::to_string),command:None})
}
fn compose_volumes(service:&Value,service_id:&str)->Vec<VolumeMount>{
    let Some(items)=service["volumes"].as_array()else{return vec![]};
    items.iter().enumerate().filter_map(|(i,item)|{
        let (source,target,read_only,kind)=if let Some(text)=item.as_str(){
            let parts:Vec<_>=text.split(':').collect();
            let (source,target,mode)=match parts.as_slice(){[target]=>("",*target,""),[source,target]=>(*source,*target,""),[source,target,mode]=>(*source,*target,*mode),_=>return None};
            let is_bind=source.starts_with('.')||source.starts_with('/')||source.starts_with('~');
            (source.to_string(),target.to_string(),mode.split(',').any(|m|m=="ro"),if is_bind{"bind"}else if source.is_empty(){"anonymous"}else{"named"})
        }else{
            let source=item["source"].as_str().unwrap_or("").to_string();let target=item["target"].as_str()?.to_string();
            let kind=item["type"].as_str().unwrap_or(if source.starts_with('.')||source.starts_with('/')||source.starts_with('~'){"bind"}else{"volume"});
            (source,target,item["read_only"].as_bool()==Some(true),kind)
        };
        Some(VolumeMount{id:format!("{service_id}-volume-{i}"),source,target,kind:kind.into(),read_only,resolution:None,disk_name:None,size_gb:None})
    }).collect()
}
pub fn analyze(path:&str,language:&str)->Result<ProjectAnalysis> {
    let root=Path::new(path).canonicalize().map_err(|_|"Select an existing project folder.")?;
    if !root.is_dir(){return Err("Select a directory.".into());}
    let name=root.file_name().and_then(|x|x.to_str()).unwrap_or("my-project").to_string();
    let all=files(&root)?;let mut services=Vec::new();let mut warnings=Vec::new();
    let compose=["compose.yaml","compose.yml","docker-compose.yml","docker-compose.yaml"].iter().find(|n|root.join(n).exists()).map(|x|x.to_string());
    if let Some(filename)=compose {
        let doc:Value=serde_yaml::from_str(&read(&root.join(&filename))).map_err(|e|format!("Invalid Compose file: {e}"))?;
        if let Some(items)=doc["services"].as_object(){for (key,s) in items {
            let image=s["image"].as_str().map(str::to_string);
            let image_name=image.as_deref().unwrap_or("").split(':').next().unwrap_or("").rsplit('/').next().unwrap_or("");
            if image_name=="certbot" {warnings.push(if language=="fa"{"سرویس certbot حذف شد؛ پس از اتصال دامنه از TLS مدیریت‌شده لیارا استفاده کنید.".into()}else{"The certbot container is omitted; use Liara-managed TLS after connecting your domain.".into()});continue;}
            let db=match image_name {"postgres"|"postgresql"=>Some("postgres"),"mongo"|"mongodb"=>Some("mongodb"),"mysql"=>Some("mysql"),"mariadb"=>Some("mariadb"),"redis"=>Some("redis"),"rabbitmq"=>Some("rabbitmq"),_=>None};
            let context=s["build"].as_str().or(s["build"]["context"].as_str()).unwrap_or(".");
            let local=scoped(&root,context)?;
            let mut service=detect(&local,context,key).unwrap_or(DetectedService{id:slug(key),name:key.clone(),kind:"app".into(),platform:"docker".into(),root:context.into(),port:80,image:None,evidence:vec![],env_keys:vec![],dependencies:vec![],warnings:vec![],volumes:vec![],build_command:None,start_command:None,command:None});
            service.kind=if db.is_some(){"database"}else{"app"}.into();service.platform=db.unwrap_or("docker").into();
            service.image=if db.is_some()||!s["build"].is_null(){None}else{image};service.evidence.push(format!("{filename}: {key}"));
            service.volumes=compose_volumes(s,key);
            service.command=s.get("command").filter(|v|!v.is_null()).cloned();
            if let Some(p)=s["ports"].as_array().and_then(|a|a.first()) {let n=p.as_str().and_then(|x|x.rsplit(':').next()).and_then(|x|x.split('/').next()).and_then(|x|x.parse::<u16>().ok()).or_else(||p["target"].as_u64().map(|x|x as u16));if let Some(n)=n{service.port=n;}}
            if let Some(kind)=db{service.port=match kind{"postgres"=>5432,"mysql"|"mariadb"=>3306,"mongodb"=>27017,"redis"=>6379,"rabbitmq"=>5672,_=>service.port};}
            service.dependencies=if let Some(a)=s["depends_on"].as_array(){a.iter().filter_map(|x|x.as_str().map(slug)).collect()}else{s["depends_on"].as_object().map(|m|m.keys().map(|x|slug(x)).collect()).unwrap_or_default()};
            if let Some(m)=s["environment"].as_object(){service.env_keys.extend(m.keys().cloned());}
            if let Some(a)=s["environment"].as_array(){service.env_keys.extend(a.iter().filter_map(|x|x.as_str()).map(|x|x.split('=').next().unwrap().into()));}
            if s.get("privileged").and_then(Value::as_bool)==Some(true)||s.get("network_mode").is_some(){service.warnings.push(if language=="fa"{"کانتینرهای privileged و حالت شبکه سفارشی در PaaS پشتیبانی نمی‌شوند.".into()}else{"Privileged containers and custom network modes are unsupported on PaaS.".into()});}
            service.env_keys.sort();service.env_keys.dedup();if service.kind=="database" {service.env_keys.clear();if !service.volumes.is_empty(){service.warnings.push(if language=="fa"{"داده‌های دیسک محلی پایگاه داده به پایگاه داده مدیریت‌شده منتقل نمی‌شوند؛ داده‌ها را جداگانه مهاجرت دهید.".into()}else{"Data in the local database volume will not be copied into the managed database; migrate it separately.".into()});}service.volumes.clear();}
            services.push(service);
        }}
        warnings.push(if language=="fa"{"سرویس‌های Compose به منابع جداگانه لیارا تبدیل می‌شوند؛ خود فایل Compose اجرا نمی‌شود.".into()}else{"Compose services are mapped to individual Liara resources; the Compose file itself is not executed.".into()});
    } else {
        let mut roots=BTreeSet::new();
        for p in &all {
            let rel=p.strip_prefix(&root).unwrap();
            if rel.components().count()<=4 && matches!(p.file_name().and_then(|x|x.to_str()),Some("package.json"|"manage.py"|"composer.json"|"go.mod"|"requirements.txt"|"Gemfile"|"Dockerfile"|"pyproject.toml")) {roots.insert(p.parent().unwrap().to_path_buf());}
        }
        if roots.is_empty(){roots.insert(root.clone());}
        for p in roots {
            let package=json_file(&p.join("package.json"));
            if p==root && package.get("workspaces").is_some() && package["scripts"]["start"].is_null(){continue;}
            let relative=p.strip_prefix(&root).unwrap().to_string_lossy().to_string();
            let service_name=if relative.is_empty(){name.clone()}else{p.file_name().unwrap().to_string_lossy().to_string()};
            if let Some(s)=detect(&p,if relative.is_empty(){"."}else{&relative},&service_name){services.push(s);}
        }
    }
    // Infer managed dependencies only from concrete package/driver evidence.
    let mut inferred=BTreeMap::<String,Vec<String>>::new();
    for service in &services {
        if service.kind!="app" {continue;}
        let dir=scoped(&root,&service.root)?;
        let manifests=format!("{} {} {} {}",read(&dir.join("package.json")),read(&dir.join("requirements.txt")),read(&dir.join("pyproject.toml")),read(&dir.join("prisma/schema.prisma"))).to_lowercase();
        for (db,signals) in [("postgres",vec!["\"pg\"","psycopg","provider = \"postgresql\""]),("mysql",vec!["mysql2","pymysql","provider = \"mysql\""]),("mongodb",vec!["mongoose","pymongo"]),("redis",vec!["ioredis","\"redis\"","redis>="])] {
            if signals.iter().any(|signal|manifests.contains(signal))&&!services.iter().any(|s|s.kind=="database"&&s.platform==db){inferred.entry(db.into()).or_default().push(service.id.clone());}
        }
    }
    for (db,parents) in inferred {
        let dep_id=format!("{}-db",db);
        for s in &mut services {if parents.contains(&s.id){s.dependencies.push(dep_id.clone());}}
        services.push(DetectedService{id:dep_id.clone(),name:dep_id,kind:"database".into(),platform:db,root:".".into(),port:0,image:None,evidence:vec!["Database driver found in project dependencies; review whether to create or reuse.".into()],env_keys:vec![],dependencies:vec![],warnings:vec![],volumes:vec![],build_command:None,start_command:None,command:None});
    }
    let mut used=BTreeSet::new();for s in &mut services {if !used.insert(s.id.clone()){s.id=format!("{}-{}",s.id,used.len());used.insert(s.id.clone());}}
    if services.is_empty(){warnings.push(if language=="fa"{"سرویس قابل استقراری پیدا نشد؛ یک فایل پیکربندی پشتیبانی‌شده یا Dockerfile اضافه کنید.".into()}else{"No deployable web service was identified. Add a supported manifest or Dockerfile.".into()});}
    let git=Command::new("git").args(["-C",root.to_str().unwrap_or(""),"rev-parse","--abbrev-ref","HEAD"]).output().ok().filter(|r|r.status.success()).map(|r|String::from_utf8_lossy(&r.stdout).trim().to_string());
    let dirty=git.is_some()&&Command::new("git").args(["-C",root.to_str().unwrap_or(""),"status","--porcelain"]).output().map(|r|!r.stdout.is_empty()).unwrap_or(false);
    Ok(ProjectAnalysis{id:id(),path:root.to_string_lossy().into(),name,services,git_branch:git,git_dirty:dirty,file_count:all.len(),warnings,analyzed_at:now()})
}
pub fn stage(root:&Path,target:&Path,generated:&[GeneratedFile])->Result<String>{
    fs::create_dir_all(target).map_err(|e|e.to_string())?;
    for file in files(root)? {let rel=file.strip_prefix(root).unwrap();let dst=target.join(rel);fs::create_dir_all(dst.parent().unwrap()).map_err(|e|e.to_string())?;fs::copy(&file,&dst).map_err(|e|e.to_string())?;}
    for f in generated {
        if !matches!(f.path.as_str(),"Dockerfile"|"liara.json"|"nginx.conf"|"Procfile"|"liara-start.py")&&!(f.path.starts_with("liara-proxy/")&&f.path.ends_with(".conf")){return Err("Agent may only generate deployment configuration files.".into());}
        let dst=scoped(target,&f.path)?;fs::create_dir_all(dst.parent().ok_or("Invalid generated path")?).map_err(|e|e.to_string())?;fs::write(dst,&f.content).map_err(|e|e.to_string())?;
    }
    source_hash(target)
}
pub fn archive(root:&Path,destination:&Path)->Result<()> {
    let writer=flate2::write::GzEncoder::new(fs::File::create(destination).map_err(|e|e.to_string())?,flate2::Compression::default());
    let mut archive=tar::Builder::new(writer);
    for file in files(root)? {let relative=file.strip_prefix(root).unwrap();archive.append_path_with_name(&file,relative).map_err(|e|e.to_string())?;}
    archive.into_inner().map_err(|e|e.to_string())?.finish().map_err(|e|e.to_string())?;
    if fs::metadata(destination).map_err(|e|e.to_string())?.len()>256*1024*1024{return Err("Source archive exceeds Liara's 256 MiB limit.".into());}Ok(())
}
pub fn local_env(root:&Path)->BTreeMap<String,String>{
    let Ok(path)=scoped(root,".env") else{return BTreeMap::new()};
    if !path.is_file(){return BTreeMap::new();}
    dotenvy::from_path_iter(path).map(|iter|iter.filter_map(|v|v.ok()).collect()).unwrap_or_default()
}
pub fn suggested_env(keys:&[String],ai:&BTreeMap<String,String>,url:&str,platform:&str)->BTreeMap<String,String>{
 let mut values=BTreeMap::new();
 for key in keys{
  let upper=key.to_uppercase();
  let sensitive=["KEY","TOKEN","SECRET","PASSWORD","PASSWD","CREDENTIAL","PRIVATE","AUTH"].iter().any(|p|upper.contains(p));
  if !sensitive{if let Some(value)=ai.get(key).filter(|v|v.len()<=2000&&!v.contains(['\n','\r'])){values.insert(key.clone(),value.clone());}}
  let default=match key.as_str(){"DEBUG"|"DJANGO_DEBUG"=>Some("False".into()),"APP_ENV"|"ENVIRONMENT"=>Some("production".into()),"APP_DEBUG"=>Some("false".into()),"APP_URL"|"SITE_URL"|"NEXTAUTH_URL"=>Some(url.into()),"ALLOWED_HOSTS"|"DJANGO_ALLOWED_HOSTS"=>Some(url.trim_start_matches("https://").into()),"CSRF_TRUSTED_ORIGINS"=>Some(url.into()),"APP_KEY" if platform=="laravel"=>{use base64::Engine;let mut bytes=Vec::from(*uuid::Uuid::new_v4().as_bytes());bytes.extend_from_slice(uuid::Uuid::new_v4().as_bytes());Some(format!("base64:{}",base64::engine::general_purpose::STANDARD.encode(bytes)))},"SECRET_KEY"|"DJANGO_SECRET_KEY"|"JWT_SECRET"|"SESSION_SECRET"|"APP_SECRET"|"AUTH_SECRET"|"NEXTAUTH_SECRET"=>Some(format!("{}{}",uuid::Uuid::new_v4().simple(),uuid::Uuid::new_v4().simple())),_=>None};
  if let Some(value)=default{values.entry(key.clone()).or_insert(value);}
 }
 values
}
pub fn snippet(root:&Path,relative:&str)->Result<String>{
    if sensitive_path(Path::new(relative)){return Err("Reading credential files is not allowed.".into());}
    let p=scoped(root,relative)?;
    if !p.is_file()||fs::metadata(&p).map_err(|e|e.to_string())?.len()>64_000{return Err("Select a text file below 64 KB.".into());}
    Ok(redact(&fs::read_to_string(p).map_err(|_|"Not a text file")?,&[]))
}
#[cfg(test)] mod tests {
 use super::*;
 #[test] fn detects_next_and_secret_names(){let d=tempfile::tempdir().unwrap();fs::write(d.path().join("package.json"),r#"{"dependencies":{"next":"15"},"scripts":{"start":"next start"}}"#).unwrap();fs::write(d.path().join(".env"),"DATABASE_URL=super-secret\n").unwrap();let a=analyze(d.path().to_str().unwrap(),"en").unwrap();assert_eq!(a.services[0].platform,"next");assert_eq!(a.services[0].env_keys,vec!["DATABASE_URL"]);assert!(!serde_json::to_string(&a).unwrap().contains("super-secret"));}
 #[test] fn maps_compose(){let d=tempfile::tempdir().unwrap();fs::write(d.path().join("compose.yml"),"services:\n  web:\n    image: nginx:alpine\n    volumes: [./nginx.conf:/etc/nginx/conf.d/default.conf:ro]\n    depends_on: [db]\n  broker:\n    image: rabbitmq:3.13-management\n  db:\n    image: postgres:16\n").unwrap();let a=analyze(d.path().to_str().unwrap(),"en").unwrap();assert!(a.services.iter().any(|s|s.kind=="database"&&s.platform=="postgres"));assert!(a.services.iter().any(|s|s.kind=="database"&&s.platform=="rabbitmq"));let nginx=a.services.iter().find(|s|s.id=="web").unwrap();assert_eq!(nginx.volumes.len(),1);assert!(nginx.volumes[0].read_only);}
 #[test] fn archive_excludes_credentials_and_symlinks(){let d=tempfile::tempdir().unwrap();let out=tempfile::tempdir().unwrap();fs::write(d.path().join("index.html"),"hello").unwrap();fs::write(d.path().join("liara.json"),r#"{"disks":[{"name":"uploads","mountTo":"/app/uploads"}]}"#).unwrap();fs::write(d.path().join(".env"),"SECRET=x").unwrap();std::os::unix::fs::symlink("/etc/passwd",d.path().join("escape")).unwrap();archive(d.path(),&out.path().join("a.gz")).unwrap();let mut tar=tar::Archive::new(flate2::read::GzDecoder::new(fs::File::open(out.path().join("a.gz")).unwrap()));let names:Vec<_>=tar.entries().unwrap().map(|e|e.unwrap().path().unwrap().into_owned()).collect();assert_eq!(names,vec![PathBuf::from("index.html"),PathBuf::from("liara.json")]);}
}

#[cfg(test)]mod environment_defaults_tests{
 use super::*;
 #[test]fn generates_application_secrets_but_never_third_party_credentials(){
  let keys=["SECRET_KEY","STRIPE_API_KEY","SMTP_PASSWORD","DEBUG","APP_URL","APP_KEY"].map(str::to_string);
  let ai=BTreeMap::from([("SECRET_KEY".into(),"made-up".into()),("STRIPE_API_KEY".into(),"invented".into()),("SMTP_PASSWORD".into(),"invented".into())]);
  let env=suggested_env(&keys,&ai,"https://my-app.liara.run","laravel");assert_eq!(env["SECRET_KEY"].len(),64);assert_ne!(env["SECRET_KEY"],"made-up");assert!(!env.contains_key("STRIPE_API_KEY"));assert!(!env.contains_key("SMTP_PASSWORD"));assert_eq!(env["APP_URL"],"https://my-app.liara.run");assert_eq!(env["DEBUG"],"False");
  use base64::Engine;assert_eq!(base64::engine::general_purpose::STANDARD.decode(env["APP_KEY"].strip_prefix("base64:").unwrap()).unwrap().len(),32);
 }
}
