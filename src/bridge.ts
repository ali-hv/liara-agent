import { invoke, isTauri } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { listen } from '@tauri-apps/api/event';
import type { DeploymentRun } from './types';

declare global {interface Window {__LIARA_TEST_INVOKE__?:(command:string,args?:Record<string,unknown>)=>Promise<unknown>}}
export const native=isTauri();
export async function call<T>(command:string,args?:Record<string,unknown>):Promise<T>{
 if(import.meta.env.DEV&&window.__LIARA_TEST_INVOKE__)return window.__LIARA_TEST_INVOKE__(command,args) as Promise<T>;
 if(!native){if(command==='app_status')return {agent:{configured:false},account:null,version:'0.1.0'} as T;if(command==='restore_account')return null as T;throw new Error('Open the desktop app with npm run desktop to use native files and connect to Liara.');}
 return invoke<T>(command,args);
}
export async function pickFolder():Promise<string|null>{if(!native&&window.__LIARA_TEST_INVOKE__)return '/test/project';if(!native)throw new Error('Folder selection is available in the native desktop app.');const result=await open({directory:true,multiple:false});return typeof result==='string'?result:null;}
export async function external(url:string){if(!/^https:\/\//.test(url))return;if(native){await invoke('open_external',{url});return;}window.open(url,'_blank','noopener,noreferrer');}
export async function subscribeRun(fn:(run:DeploymentRun)=>void){if(!native)return ()=>{};return listen<DeploymentRun>('deployment:update',event=>fn(event.payload));}
export function errorText(error:unknown){return error instanceof Error?error.message:String(error);}
