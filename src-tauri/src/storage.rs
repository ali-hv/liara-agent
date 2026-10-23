use crate::{models::*};
use serde_json::Value;

// Preserve the current compute capacity, region and bundle while satisfying storage.
#[derive(Debug,PartialEq)]pub enum ResizeStep{Ready,Submit,Uncertain}
pub fn resize_step(actual:&str,from:&str,target:&str,pending:Option<&str>)->Result<ResizeStep>{
 if actual==target{return Ok(ResizeStep::Ready);}
 if actual!=from{return Err("The application plan changed after review. Request a new recommendation.".into());}
 if pending==Some(format!("resize:{target}").as_str()){return Ok(ResizeStep::Uncertain);}
 if pending.is_some(){return Err("An unfinished request must be reconciled before upgrading this app.".into());}
 Ok(ResizeStep::Submit)
}
pub fn fitting_plan<'a>(plans:&'a [PlanOption],current:&PlanOption,platform:&str,required:f64)->Option<&'a PlanOption>{
 plans.iter().filter(|p|p.category=="app"&&p.bundle_id==current.bundle_id&&p.region==current.region&&p.ram>=current.ram&&p.cpu>=current.cpu&&p.disk>=required&&p.monthly_price.is_some()&&(p.supports.is_empty()||p.supports.iter().any(|s|s==platform)))
 .min_by(|a,b|a.monthly_price.unwrap().total_cmp(&b.monthly_price.unwrap()).then_with(||a.disk.total_cmp(&b.disk)).then_with(||a.id.cmp(&b.id)))
}
pub fn required_storage(volumes:&[VolumeMount],existing:&[Value])->Result<f64>{
 let mut disks=std::collections::BTreeMap::<String,f64>::new();
 for disk in existing{let name=disk["name"].as_str().ok_or("Existing disk name unavailable.")?;let size=disk["size"].as_f64().filter(|s|s.is_finite()&&*s>=0.0).ok_or("Existing disk size unavailable. Review storage in Liara console.")?;disks.insert(name.into(),size);}
 for volume in volumes.iter().filter(|v|v.resolution.as_deref()==Some("disk")){let name=volume.disk_name.as_deref().ok_or("Approved disk name missing")?;let size=volume.size_gb.ok_or("Approved disk size missing")? as f64;disks.entry(name.into()).and_modify(|n|*n=n.max(size)).or_insert(size);}
 Ok(disks.values().sum())
}
pub fn resize_advice(current:&PlanOption,target:&PlanOption,required:f64)->Result<AppResize>{
 Ok(AppResize{from_plan_id:current.id.clone(),from_monthly_price:current.monthly_price.ok_or("Current plan pricing is unavailable. Refresh the catalog before approving an upgrade.")?,required_gb:required,target_capacity_gb:target.disk,target_monthly_price:target.monthly_price.ok_or("Recommended plan pricing is unavailable.")?})
}
pub fn explain(service:&ServicePlan,language:&str)->String{
 let r=service.resize.as_ref().unwrap();
 if language=="fa"{format!("برای مجموع {} گیگابایت فضای فایل، پلن {} از {} به {} ارتقا می‌یابد. هزینه ماهانه این برنامه از {:.0} به {:.0} تومان تغییر می‌کند. برنامه و دیسک‌های موجود حفظ می‌شوند.",r.required_gb,service.name,r.from_plan_id,service.plan_id,r.from_monthly_price,r.target_monthly_price)}else{format!("For {} GB of file storage, upgrade {} from {} to {}. This app's monthly estimate changes from {:.0} to {:.0} tomans. Existing app and disks are retained.",r.required_gb,service.name,r.from_plan_id,service.plan_id,r.from_monthly_price,r.target_monthly_price)}
}
#[cfg(test)]mod tests{
 use super::*;
 fn plan(id:&str,disk:f64,price:Option<f64>)->PlanOption{PlanOption{id:id.into(),category:"app".into(),label:id.into(),ram:1.0,cpu:1.0,disk,monthly_price:price,bundle_id:"basic".into(),region:"iran".into(),supports:vec!["docker".into()]}}
 #[test]fn selects_cheapest_compatible_sufficient_plan_and_never_downgrades_compute(){let old=plan("small",5.0,Some(100.0));let mut bad=plan("cheap",10.0,Some(1.0));bad.ram=0.5;let mut foreign=plan("elsewhere",10.0,Some(2.0));foreign.region="elsewhere".into();let mut bundle=plan("other-bundle",10.0,Some(3.0));bundle.bundle_id="free".into();let list=vec![old.clone(),plan("large",20.0,Some(300.0)),plan("medium",10.0,Some(200.0)),bad,foreign,bundle,plan("unknown",10.0,None)];assert_eq!(fitting_plan(&list,&old,"docker",7.0).unwrap().id,"medium");assert!(fitting_plan(&list,&old,"docker",100.0).is_none());assert!(fitting_plan(&list,&old,"next",7.0).is_none());}
 #[test]fn interrupted_resizes_are_reconciled_instead_of_resubmitted(){
  assert_eq!(resize_step("small","small","medium",None).unwrap(),ResizeStep::Submit);
  assert_eq!(resize_step("small","small","medium",Some("resize:medium")).unwrap(),ResizeStep::Uncertain);
  assert_eq!(resize_step("medium","small","medium",Some("resize:medium")).unwrap(),ResizeStep::Ready);
  assert!(resize_step("other","small","medium",None).is_err());assert!(resize_step("small","small","medium",Some("disk:media")).is_err());
 }
 #[test]fn both_prices_must_be_known_before_approval(){let old=plan("small",5.0,None);let next=plan("medium",10.0,Some(200.0));assert!(resize_advice(&old,&next,7.0).is_err());}
}
