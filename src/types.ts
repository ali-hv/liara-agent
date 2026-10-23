export interface PlanOption {id:string;category:string;label:string;ram:number;cpu:number;disk:number;monthlyPrice:number|null;bundleId:string;region:string;supports:string[]}
export interface Account {id:string;name:string;email:string;balance:number|null;credentialStorage:string;plans:PlanOption[];networks:Record<string,unknown>[];projects:Record<string,unknown>[];databases:Record<string,unknown>[];buckets:Record<string,unknown>[];databaseVersions:Record<string,string>;warnings:string[]}
export interface VolumeMount {id:string;source:string;target:string;kind:string;readOnly:boolean;resolution:string|null;diskName:string|null;sizeGb:number|null}
export interface VolumeChoice {resolution?:string;diskName?:string;sizeGb?:number}
export interface DetectedService {id:string;name:string;kind:string;platform:string;root:string;port:number;image:string|null;evidence:string[];envKeys:string[];dependencies:string[];warnings:string[];volumes:VolumeMount[];command:unknown;buildCommand:string|null;startCommand:string|null}
export interface ProjectAnalysis {id:string;path:string;name:string;services:DetectedService[];gitBranch:string|null;gitDirty:boolean;fileCount:number;warnings:string[];analyzedAt:string}
export interface GeneratedFile {path:string;content:string;reason:string}
export interface AppResize {fromPlanId:string;fromMonthlyPrice:number;targetMonthlyPrice:number;requiredGb:number;targetCapacityGb:number}
export interface ServicePlan {resize?:AppResize|null;id:string;name:string;kind:string;platform:string;root:string;port:number;image:string|null;planId:string;bundleId:string;monthlyPrice:number|null;existingId:string|null;version:string|null;localEnvKeys:string[];envKeys:string[];bindings:Record<string,string>;dependencies:string[];volumes:VolumeMount[];command:unknown;files:GeneratedFile[];sourceHash:string;rationale:string}
export interface DeploymentPlan {id:string;projectId:string;accountId:string;revision:string;network:string|null;services:ServicePlan[];summary:string;warnings:string[];blockers:string[];createdAt:string;language:string;allowConfigRepairs:boolean}
export interface ApprovalRequest {id:string;planId:string;revision:string;actions:string[];monthlyPrice:number|null;expiresAt:string}
export interface RunEvent {at:string;level:string;serviceId:string|null;message:string}
export interface ResourceProgress {serviceId:string;resourceId:string|null;sourceId:string|null;releaseId:string|null;phase:string;url:string|null;repairAttempts:number;requestPending:string|null}
export interface DeploymentRun {id:string;plan:DeploymentPlan;status:string;startedAt:string;updatedAt:string;networkId:string|null;networkPending:boolean;resources:ResourceProgress[];events:RunEvent[]}
export interface ServiceChoice {planLocked?:boolean;name?:string;planId?:string;bundleId?:string;existingId?:string;port?:number;version?:string;bindings:Record<string,string>;volumes:Record<string,VolumeChoice>}
export interface PlanningQuestion {id:string;question:string;hint:string;options:string[]}
export interface PlanChoices {answers?:Record<string,string>;guided?:boolean;services:Record<string,ServiceChoice>;network?:string;allowConfigRepairs:boolean}
export interface ChatMessage {role:'user'|'assistant';content:string}
export type Secrets=Record<string,Record<string,string>>;
export interface AppStatus {agent:{configured:boolean;model?:string;baseUrl?:string;message?:string};account:Account|null;version:string}
export interface DomainRequest {id:string;domain:string;projectName:string;actions:string[]}
export interface DomainResult {domain:string;domainId:string;target:string;status:string;instructions:string;tls:{requested:boolean;reason?:string;error?:string}}

export interface ProjectFixReview {id:string;planId:string;revision:string;explanation:string;changes:{path:string;before:string;after:string;reason:string}[];expiresAt:string}
