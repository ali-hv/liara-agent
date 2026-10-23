import {test,expect,type Page} from '@playwright/test';
async function mockNative(page:Page,withVolumes=false,language:'en'|'fa'|'default'='en',failBalance=false){await page.addInitScript(({withVolumes,language,failBalance})=>{if(language==='default'){localStorage.removeItem('liara-language');localStorage.removeItem('liara-language-selected');}else{localStorage.setItem('liara-language',language);localStorage.setItem('liara-language-selected','yes');}
 const planOption={id:'g2-test',category:'app',label:'Test plan',ram:1,cpu:1,disk:10,monthlyPrice:120000,bundleId:'free',region:'iran',supports:[]};
 const account={id:'account-test',name:'Test Account',email:'test@example.com',balance:500000,credentialStorage:'session',plans:[planOption,{...planOption,id:'g2-large',ram:2,monthlyPrice:240000}],networks:[{_id:'network-1',name:'Test network'}],projects:[],databases:[],buckets:[],databaseVersions:{postgres:'16'},warnings:[]};
 const service={id:'web',name:'web',kind:'app',platform:'next',root:'.',port:3000,image:null,evidence:['package.json','Next.js dependency'],envKeys:['SECRET_KEY'],dependencies:[],warnings:[],volumes:withVolumes?[{id:'web-volume-0',source:'uploads',target:'/app/uploads',kind:'volume',readOnly:false,resolution:null,diskName:null,sizeGb:null}]:[],command:null,buildCommand:'next build',startCommand:'next start'};
 const project={id:'project-test',name:'test-project',path:'/test/project',services:[service],gitBranch:'main',gitDirty:false,fileCount:24,warnings:[],analyzedAt:new Date().toISOString()};
 let projectHidden=false;let connected=false;let selected='g2-test';let selectedVolume:{resolution?:string;diskName?:string;sizeGb?:number}|undefined;let lastRun:unknown=null;
 window.__LIARA_TEST_INVOKE__=async(command,args)=>{
  switch(command){
   case 'app_status':return {account:connected?account:null,agent:{configured:true,model:'TEST MODEL'},version:'0.1.0'};
   case 'restore_account':return null;
   case 'connect_account':if(args?.token==='invalid')throw Error('HTTP 401: Token was rejected.');(window as any).__LIARA_TEST_TEAM_ID__=args?.teamId;connected=true;return account;
   case 'refresh_account':return account;
   case 'plan_environment':return {};
   case 'disconnect_account':connected=false;return null;
   case 'list_projects':return connected&&!projectHidden?[project]:[];
   case 'remove_recent_project':projectHidden=true;return null;
   case 'delete_deployment':lastRun=null;return null;
   case 'list_runs':return lastRun?[lastRun]:[];
   case 'scan_project':projectHidden=false;return project;
   case 'planning_questions':return [{id:'audience',question:language==='fa'?'چند نفر از سایت استفاده می‌کنند؟':'How many people will use your website?',hint:language==='fa'?'تخمین روزانه کافی است.':'A daily estimate is enough.',options:language==='fa'?['فقط خودم','تا ۱۰۰ نفر']:['Just me','Up to 100 per day']}];
   case 'prepare_plan':{(window as any).__PLANNING_CHOICES__=args?.choices;const choice=(args?.choices as {services?:Record<string,{planId?:string;volumes?:Record<string,{resolution?:string;diskName?:string;sizeGb?:number}>}>})?.services?.web;selected=choice?.planId||'g2-test';selectedVolume=choice?.volumes?.['web-volume-0'];const volumes=withVolumes?[{...service.volumes[0],resolution:selectedVolume?.resolution||null,diskName:selectedVolume?.diskName||null,sizeGb:selectedVolume?.sizeGb||null}]:[];return {id:'plan-test',projectId:project.id,accountId:account.id,revision:'revision-test',network:'network-1',services:[{...service,name:'test-web',planId:selected,bundleId:'free',monthlyPrice:selected==='g2-test'?120000:240000,existingId:null,version:null,localEnvKeys:[],bindings:{},volumes,command:null,files:[{path:'liara.json',content:'{"app":"test-web"}',reason:'Test staging config'}],sourceHash:'test-hash',rationale:'SIMULATED: a small plan is sufficient for this fixture.'}],summary:'SIMULATED TEST DATA — no Liara resources will be created.',warnings:[],blockers:withVolumes&&!selectedVolume?.resolution?['web: Choose a Compose volume action']:[],createdAt:new Date().toISOString(),language:'en',allowConfigRepairs:false};}
   case 'request_approval':return {id:'approval-test',planId:'plan-test',revision:'revision-test',actions:['SIMULATED create app','SIMULATED deploy',...(selectedVolume?.resolution==='disk'?[`SIMULATED persistent disk ${selectedVolume.diskName} (${selectedVolume.sizeGb} GB)`]:[])],monthlyPrice:selected==='g2-test'?120000:240000,expiresAt:new Date(Date.now()+900000).toISOString()};
   case 'start_deployment':lastRun={id:'run-test',plan:await window.__LIARA_TEST_INVOKE__('prepare_plan',{}),status:failBalance?'failed':'succeeded',startedAt:new Date().toISOString(),updatedAt:new Date().toISOString(),resources:[{serviceId:'web',resourceId:'test-id',sourceId:'test-source',releaseId:'test-release',phase:failBalance?'failed':'verified',url:'https://example.com',repairAttempts:0,requestPending:null}],events:[{at:new Date().toISOString(),level:failBalance?'error':'success',serviceId:'web',message:failBalance?'HTTP 402: Your Liara balance is insufficient. Not enough balance.':'SIMULATED verification; no deployment occurred.'}]};return lastRun;
   case 'chat_agent':return 'SIMULATED agent response. No external AI request occurred.';
   default:throw new Error(`Unexpected test command: ${command}`);
  }
 };
 },{withVolumes,language,failBalance});}
async function connect(page:Page,token='test-token'){await page.getByLabel('Liara API token',{exact:true}).fill(token);await page.getByLabel('I understand how project analysis uses AI.').check();await page.getByRole('button',{name:'Connect account',exact:true}).click();}

async function startPlanning(page:Page,language:'en'|'fa'='en',advanced=true){
 await page.getByRole('button',{name:language==='fa'?'برنامه‌ریزی استقرار با دستیار':'Let the assistant plan deployment'}).click();
 await page.getByRole('dialog').getByRole('combobox').click();await page.getByRole('option',{name:language==='fa'?'فقط خودم':'Just me',exact:true}).click();
 await page.getByRole('button',{name:language==='fa'?'آماده کردن پیشنهاد استقرار':'Create deployment plan',exact:true}).click();
 if(advanced)await page.getByRole('button',{name:language==='fa'?'تغییر تنظیمات استقرار':'Change deployment settings',exact:true}).click();
}
test('rejects invalid tokens without entering the workspace',async({page})=>{await mockNative(page);await page.goto('/');await connect(page,'invalid');await expect(page.getByRole('alert')).toContainText('401');await expect(page.getByRole('heading',{name:'Let’s connect your account'})).toBeVisible();});
test('Persian is the default, the language toggle persists, and tutorial and errors are localized',async({page})=>{await mockNative(page,false,'default');await page.goto('/');await expect(page.locator('html')).toHaveAttribute('lang','fa');await expect(page.locator('html')).toHaveAttribute('dir','rtl');await expect(page.getByRole('heading',{name:'حسابتان را متصل کنیم'})).toBeVisible();await expect.poll(()=>page.evaluate(()=>getComputedStyle(document.body).fontFamily)).toContain('Yekan');await page.getByRole('button',{name:/توکن API، در یک دقیقه/}).click();const video=page.locator('video');await expect(video).toBeVisible();await expect.poll(()=>video.evaluate((v:HTMLVideoElement)=>v.readyState)).toBeGreaterThan(0);await page.getByRole('dialog').getByRole('button',{name:'بستن',exact:true}).click();await expect(page.getByLabel('شناسه تیم (اختیاری)')).toBeVisible();await expect(page.getByText(/شناسه تیم مشخص می‌کند/)).toBeVisible();await page.getByRole('button',{name:/پیدا کردن شناسه تیم/}).click();await expect(page.locator('video')).toHaveAttribute('src','/reach-team-id-using-console.mp4');await expect.poll(()=>page.locator('video').evaluate((v:HTMLVideoElement)=>v.readyState)).toBeGreaterThan(0);await page.getByRole('dialog').getByRole('button',{name:'بستن',exact:true}).click();await page.getByRole('button',{name:'زبان',exact:true}).click();await expect(page.locator('html')).toHaveAttribute('dir','ltr');await expect(page.getByRole('heading',{name:'Let’s connect your account'})).toBeVisible();await page.getByRole('button',{name:'Language',exact:true}).click();await expect(page.locator('html')).toHaveAttribute('dir','rtl');expect(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth)).toBe(false);await page.screenshot({path:'docs/screenshots/rtl-verified.png',fullPage:true});await page.getByLabel('توکن API لیارا',{exact:true}).fill('invalid');await page.getByLabel('با نحوه استفاده از هوش مصنوعی برای تحلیل پروژه آشنا هستم.').check();await page.getByRole('button',{name:'اتصال حساب',exact:true}).click();await expect(page.getByRole('alert')).toContainText('انجام عملیات ممکن نشد');await page.getByRole('dialog').getByRole('button',{name:'بستن',exact:true}).first().click();await page.getByText('جزئیات فنی',{exact:true}).click();await expect(page.getByRole('alert')).toContainText('Token was rejected');});
test('project → alternative plan → required secret → approval → simulated result',async({page})=>{await mockNative(page);await page.goto('/');await connect(page);await expect(page.getByRole('heading',{name:'What are we launching?'})).toBeVisible();await page.getByRole('button',{name:/Your project starts here/}).click();await expect(page.getByRole('heading',{name:'test-project',exact:true})).toBeVisible();await startPlanning(page);await expect(page.getByText('SIMULATED: a small plan is sufficient for this fixture.',{exact:true})).toBeVisible();await expect(page.getByRole('button',{name:'Update deployment plan'})).toBeVisible();await expect(page.getByRole('button',{name:'Review & deploy'})).toBeDisabled();await page.getByRole('combobox',{name:'Resource plan',exact:true}).click();await page.locator('[role=option][data-value="g2-large|free"]').click();await expect(page.getByRole('button',{name:'Update deployment plan'})).toBeVisible();await page.getByRole('button',{name:'Update deployment plan'}).click();expect(await page.evaluate(()=>(window as any).__PLANNING_CHOICES__.services.web.planLocked)).toBe(true);await page.getByLabel('SECRET_KEY',{exact:true}).fill('not-a-real-secret');await page.screenshot({path:'docs/screenshots/review-test.png',fullPage:true});await page.getByRole('button',{name:'Review & deploy'}).click();await expect(page.getByRole('dialog')).toContainText('240,000');await page.getByRole('button',{name:'Approve & deploy',exact:true}).click();await expect(page.getByRole('heading',{name:'Hello, world.'})).toBeVisible();await expect(page.getByText('SIMULATED verification; no deployment occurred.')).toBeVisible();expect(await page.evaluate(()=>JSON.stringify(localStorage))).not.toContain('not-a-real-secret');await page.screenshot({path:'docs/screenshots/success-test.png',fullPage:true});});
test('volume blocker is actionable and a disk appears in the approval',async({page})=>{await mockNative(page,true);await page.goto('/');await connect(page);await page.getByRole('button',{name:/Your project starts here/}).click();await startPlanning(page);await expect(page.getByText('Choose a Compose volume action')).toBeVisible();await expect(page.getByRole('button',{name:'Review & deploy'})).toBeDisabled();await page.getByRole('combobox',{name:'Storage handling',exact:true}).click();await page.locator('[role=option][data-value="disk"]').click();await page.getByLabel('Liara disk name').fill('uploads-data');await page.getByLabel('Disk size (GB)').fill('10');await page.getByRole('button',{name:'Update deployment plan'}).click();await page.getByLabel('SECRET_KEY',{exact:true}).fill('not-a-real-secret');await expect(page.getByText('Disks use the app plan’s storage allowance; existing disks count toward the total.')).toBeVisible();await page.getByRole('button',{name:'Review & deploy'}).click();await expect(page.getByRole('dialog')).toContainText('uploads-data (10 GB)');});


test('Liara console button opens the requested browser page',async({page})=>{await mockNative(page);await page.goto('/');await page.context().route('https://console.liara.ir/**',route=>route.fulfill({status:200,contentType:'text/html',body:'<title>Liara console test</title>'}));const popupEvent=page.waitForEvent('popup');await page.getByRole('button',{name:'Open Liara console',exact:true}).click();const popup=await popupEvent;await expect.poll(()=>popup.url()).toContain('https://console.liara.ir/');});
test('optional Team ID is passed when connecting and blank uses the personal account',async({page})=>{await mockNative(page);await page.goto('/');await page.getByLabel('Team ID (optional)').fill('team-123');await connect(page);expect(await page.evaluate(()=>(window as any).__LIARA_TEST_TEAM_ID__)).toBe('team-123');});
test('insufficient balance opens a localized top-up dialog with a console action',async({page})=>{await mockNative(page,false,'fa',true);await page.goto('/');await page.getByLabel('توکن API لیارا',{exact:true}).fill('test-token');await page.getByLabel('با نحوه استفاده از هوش مصنوعی برای تحلیل پروژه آشنا هستم.').check();await page.getByRole('button',{name:'اتصال حساب',exact:true}).click();await page.getByRole('button',{name:/پروژه شما از اینجا شروع می‌شود/}).click();await startPlanning(page,'fa');await page.getByLabel('SECRET_KEY',{exact:true}).fill('not-a-real-secret');await page.getByRole('button',{name:'بررسی و استقرار'}).click();await page.getByRole('button',{name:'تأیید و استقرار'}).click();await expect(page.getByRole('dialog')).toContainText('موجودی حساب لیارا کافی نیست');await expect(page.getByRole('dialog').getByRole('button',{name:'شارژ حساب در کنسول لیارا'})).toBeVisible();});
test('dark theme and collapsed assistant remain usable at minimum desktop size',async({page})=>{await mockNative(page);await page.setViewportSize({width:1000,height:700});await page.goto('/');await page.getByRole('button',{name:'Switch theme'}).click();await expect(page.locator('html')).toHaveAttribute('data-theme','dark');await page.getByRole('button',{name:'Hide assistant',exact:true}).first().click();await expect(page.locator('.assistant-panel')).toHaveCount(0);expect(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth)).toBe(false);});

test('Persian deployment estimate stays readable at desktop widths',async({page})=>{
 await mockNative(page,false,'fa');await page.goto('/');
 await page.getByLabel('توکن API لیارا',{exact:true}).fill('test-token');
 await page.getByLabel('با نحوه استفاده از هوش مصنوعی برای تحلیل پروژه آشنا هستم.').check();
 await page.getByRole('button',{name:'اتصال حساب',exact:true}).click();
 await page.getByRole('button',{name:/پروژه شما از اینجا شروع می‌شود/}).click();
 await startPlanning(page,'fa');
 for(const width of [1440,1000]){
  await page.setViewportSize({width,height:1000});
  await expect(page.locator('.cost-card')).toBeVisible();
  expect(await page.locator('.cost-number').evaluate(el=>parseFloat(getComputedStyle(el).fontSize))).toBeGreaterThanOrEqual(36);
  expect(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth)).toBe(false);
 }
 await page.screenshot({path:'docs/screenshots/review-fa-spacing.png',fullPage:true});
});

test('local env values are editable, collapsible, and errors open a dialog and scroll to the banner',async({page})=>{
 await mockNative(page);
 await page.addInitScript(()=>{const original=window.__LIARA_TEST_INVOKE__!;window.__LIARA_TEST_INVOKE__=async(command,args)=>{
  if(command==='plan_environment')return {web:{SECRET_KEY:'local-fixture-value'}};
  if(command==='request_approval'){(window as any).__SUBMITTED_ENVS__=args?.secrets;throw new Error('HTTP 404: Not Found [GET /v1/projects/test-web]')}
  return original(command,args);
 }});
 await page.goto('/');await connect(page);
 await page.getByRole('button',{name:/Your project starts here/}).click();
 await startPlanning(page);
 const field=page.getByLabel('SECRET_KEY',{exact:true});await expect(field).toHaveValue('local-fixture-value');await expect(field).toHaveAttribute('type','text');
 await field.fill('edited-fixture-value');
 const section=page.locator('details.env-panel');await section.locator('summary').click();await expect(field).not.toBeVisible();
 await section.locator('summary').click();await expect(field).toHaveValue('edited-fixture-value');
 await page.getByRole('button',{name:'Review & deploy'}).click();
 await expect(page.getByRole('dialog')).toContainText('Something needs attention');
 expect(await page.evaluate(()=>(window as any).__SUBMITTED_ENVS__.web.SECRET_KEY)).toBe('edited-fixture-value');
 await page.getByRole('dialog').getByRole('button',{name:'Close',exact:true}).first().click();
 await expect(page.getByRole('alert')).toContainText('404');
 expect(await page.getByRole('alert').evaluate(el=>el.getBoundingClientRect().top)).toBeLessThan(100);
 expect(await page.evaluate(()=>JSON.stringify(localStorage))).not.toContain('fixture-value');
});

test('resume opens approval directly without repeating the environment form',async({page})=>{
 await mockNative(page,false,'en',true);await page.goto('/');await connect(page);
 await page.getByRole('button',{name:/Your project starts here/}).click();
 await startPlanning(page);
 await page.getByLabel('SECRET_KEY',{exact:true}).fill('session-fixture-value');
 await page.getByRole('button',{name:'Review & deploy'}).click();
 await page.getByRole('button',{name:'Approve & deploy',exact:true}).click();
 await expect(page.getByRole('dialog')).toBeVisible();
 await page.getByRole('dialog').getByRole('button',{name:'Close',exact:true}).first().click();
 await page.getByRole('button',{name:'Review & resume',exact:true}).click();
 await expect(page.getByRole('button',{name:'Approve & resume',exact:true})).toBeVisible();
 await expect(page).toHaveURL(/\/run$/);
 await expect(page.getByLabel('SECRET_KEY',{exact:true})).toHaveCount(0);
});

test('guided Persian planning asks questions and keeps technical settings optional',async({page})=>{
 await mockNative(page,false,'fa');
 await page.addInitScript(()=>{const original=window.__LIARA_TEST_INVOKE__!;window.__LIARA_TEST_INVOKE__=async(command,args)=>command==='plan_environment'?{web:{SECRET_KEY:'local-fixture'}}:original(command,args)});
 await page.goto('/');await page.getByLabel('توکن API لیارا',{exact:true}).fill('test-token');
 await page.getByLabel('با نحوه استفاده از هوش مصنوعی برای تحلیل پروژه آشنا هستم.').check();
 await page.getByRole('button',{name:'اتصال حساب',exact:true}).click();
 await page.getByRole('button',{name:/پروژه شما از اینجا شروع می‌شود/}).click();
 await page.getByRole('button',{name:'برنامه‌ریزی استقرار با دستیار'}).click();
 await expect(page.getByRole('dialog')).toContainText('چند نفر از سایت استفاده می‌کنند؟');
 await page.screenshot({path:'docs/screenshots/workload-questions-fa.png',fullPage:true});
 await page.getByRole('dialog').getByRole('combobox').click();await page.getByRole('option',{name:'فقط خودم',exact:true}).click();
 await page.getByRole('button',{name:'آماده کردن پیشنهاد استقرار',exact:true}).click();
 await expect(page.getByRole('heading',{name:'پیشنهاد ما برای پروژه شما'})).toBeVisible();
 expect(await page.evaluate(()=>(window as any).__PLANNING_CHOICES__.answers.audience)).toBe('فقط خودم');
 expect(await page.evaluate(()=>(window as any).__PLANNING_CHOICES__.guided)).toBe(true);
 await expect(page.locator('.service-plans')).not.toBeVisible();
 await expect(page.locator('.env-panel')).toHaveCount(0);
 await expect(page.getByRole('button',{name:'استقرار پروژه',exact:true})).toBeEnabled();
 await page.screenshot({path:'docs/screenshots/simple-review-fa.png',fullPage:true});
 await page.getByRole('button',{name:'تغییر تنظیمات استقرار',exact:true}).click();
 await expect(page.getByLabel('SECRET_KEY',{exact:true})).toHaveValue('local-fixture');
 await expect(page.locator('.service-plans')).toBeVisible();
 await page.getByRole('button',{name:'بازگشت به نمایش ساده',exact:true}).click();
 await expect(page.locator('.service-plans')).not.toBeVisible();
 await page.getByRole('button',{name:'استقرار پروژه',exact:true}).click();
 await expect(page.getByRole('button',{name:'تأیید و استقرار',exact:true})).toBeVisible();
});

test('remove a recent project with confirmation and select its folder again',async({page})=>{
 await mockNative(page);await page.goto('/');await connect(page);
 await page.getByRole('button',{name:'Remove recent project: test-project',exact:true}).first().click();
 await expect(page.getByRole('dialog')).toContainText('Your files, deployment history and Liara resources remain');
 await page.getByRole('dialog').getByRole('button',{name:'Go back',exact:true}).click();
 await expect(page.locator('.recent-card')).toHaveCount(1);
 await page.getByRole('button',{name:'Remove recent project: test-project',exact:true}).first().click();
 await page.getByRole('button',{name:'Remove local record',exact:true}).click();
 await expect(page.locator('.recent-card')).toHaveCount(0);await expect(page.locator('.project-nav-row')).toHaveCount(0);
 await page.getByRole('button',{name:/Your project starts here/}).click();
 await expect(page.getByRole('heading',{name:'test-project',exact:true})).toBeVisible();
 await expect(page.locator('.project-nav-row')).toHaveCount(1);
});

test('delete deployment history with a clear warning that cloud resources remain',async({page})=>{
 await mockNative(page);await page.goto('/');await connect(page);await page.getByRole('button',{name:/Your project starts here/}).click();
 await startPlanning(page);await page.getByLabel('SECRET_KEY',{exact:true}).fill('fixture-value');
 await page.getByRole('button',{name:'Review & deploy',exact:true}).click();await page.getByRole('button',{name:'Approve & deploy',exact:true}).click();
 await expect(page.getByRole('heading',{name:'Hello, world.'})).toBeVisible();
 await page.getByRole('button',{name:/^Deployments/}).click();
 await page.getByRole('button',{name:'Delete deployment history: test-web',exact:true}).click();
 await expect(page.getByRole('dialog')).toContainText('may continue to cost money');
 await page.getByRole('button',{name:'Remove local record',exact:true}).click();
 await expect(page.locator('.history-card')).toHaveCount(0);await expect(page.locator('.empty-state')).toBeVisible();
 await expect(page.locator('.project-nav-row')).toHaveCount(1);
});

test('custom dropdown supports keyboard selection, escape, and dark theme',async({page})=>{
 await mockNative(page);await page.goto('/');await connect(page);await page.getByRole('button',{name:/Your project starts here/}).click();
 await page.getByRole('button',{name:'Switch theme',exact:true}).click();
 await page.getByRole('button',{name:'Let the assistant plan deployment',exact:true}).click();
 const combo=page.getByRole('dialog').getByRole('combobox');await combo.focus();await combo.press('ArrowDown');await expect(page.getByRole('listbox')).toBeVisible();
 await page.screenshot({path:'docs/screenshots/dropdown-dark.png',fullPage:true});
 await combo.press('ArrowDown');await combo.press('Enter');await expect(combo).toContainText('Up to 100 per day');
 await combo.press('Enter');await combo.press('Escape');await expect(page.getByRole('listbox')).toHaveCount(0);await expect(page.getByRole('dialog')).toBeVisible();
 await page.getByRole('button',{name:'Create deployment plan',exact:true}).click();
 await expect(page.locator('.page-heading>p')).toHaveText('We selected resources for your project. Check the cost and deploy when ready.');
 expect(await page.locator('.page-heading>p').evaluate(el=>parseFloat(getComputedStyle(el).fontSize))).toBeGreaterThanOrEqual(16);
});

test('automatic environment values stay editable while third-party credentials are required',async({page})=>{
 await mockNative(page);
 await page.addInitScript(()=>{const original=window.__LIARA_TEST_INVOKE__!;window.__LIARA_TEST_INVOKE__=async(command,args)=>{
  if(command==='plan_environment')return {web:{SECRET_KEY:'generated-local-fixture',DEBUG:'False'}};
  const result=await original(command,args);
  if(command==='prepare_plan'){(result as any).services[0].envKeys=['SECRET_KEY','DEBUG','STRIPE_API_KEY'];}
  if(command==='request_approval')(window as any).__SUBMITTED_ENVS__=args?.secrets;
  return result;
 }});
 await page.goto('/');await connect(page);await page.getByRole('button',{name:/Your project starts here/}).click();await startPlanning(page);
 await expect(page.getByLabel('SECRET_KEY',{exact:true})).toHaveValue('generated-local-fixture');
 await expect(page.getByLabel('DEBUG',{exact:true})).toHaveValue('False');
 await expect(page.getByLabel('STRIPE_API_KEY',{exact:true})).toHaveValue('');
 await expect(page.getByRole('button',{name:'Review & deploy',exact:true})).toBeDisabled();
 await page.getByLabel('DEBUG',{exact:true}).fill('custom-production-value');await page.getByLabel('STRIPE_API_KEY',{exact:true}).fill('external-fixture');
 await page.getByRole('button',{name:'Review & deploy',exact:true}).click();
 expect(await page.evaluate(()=>(window as any).__SUBMITTED_ENVS__.web.DEBUG)).toBe('custom-production-value');
 expect(await page.evaluate(()=>JSON.stringify(localStorage))).not.toContain('generated-local-fixture');
});

test('simple review explains genuine unresolved setup without a technical red warning list',async({page})=>{
 await mockNative(page,true);await page.goto('/');await connect(page);await page.getByRole('button',{name:/Your project starts here/}).click();await startPlanning(page,'en',false);
 await expect(page.locator('.readiness-note')).toBeVisible();await expect(page.locator('.notice.error')).toHaveCount(0);
 await expect(page.getByRole('button',{name:'Deploy project',exact:true})).toBeDisabled();
 await page.getByRole('button',{name:'Review the required settings',exact:true}).click();
 await expect(page.locator('.notice.error')).toContainText('Choose a Compose volume action');
 await expect(page.getByRole('combobox',{name:'Storage handling',exact:true})).toBeVisible();
});


test('storage recovery recommends a sufficient plan in Persian and upgrades only after cost approval',async({page})=>{
 await mockNative(page,false,'fa');
 await page.addInitScript(()=>{const original=window.__LIARA_TEST_INVOKE__!;let starts=0;let recommendation:any=null;window.__LIARA_TEST_INVOKE__=async(command,args)=>{
  if(command==='prepare_storage_recovery'){
   const plan=await original('prepare_plan',{}) as any;recommendation={...plan,id:'storage-recovery',revision:'resize-revision',summary:'پلن مناسب فایل‌های شما انتخاب شد.',services:plan.services.map((s:any)=>({...s,existingId:'test-id',planId:'g2-large',monthlyPrice:240000,resize:{fromPlanId:'g2-test',fromMonthlyPrice:120000,targetMonthlyPrice:240000,requiredGb:7,targetCapacityGb:10}}))};return recommendation;
  }
  if(command==='request_approval'&&args?.planId==='storage-recovery'){
   (window as any).__RECOVERY_APPROVAL_ARGS__=args;
   return {id:'upgrade-approval',planId:'storage-recovery',revision:'resize-revision',actions:['ارتقای برنامه موجود از g2-test به g2-large برای ۷ گیگابایت فضا؛ هزینه از ۱۲۰۰۰۰ به ۲۴۰۰۰۰ تومان'],monthlyPrice:240000,expiresAt:new Date(Date.now()+900000).toISOString()};
  }
  const value=await original(command,args);
  if(command==='start_deployment'){starts++;(window as any).__DEPLOY_STARTS__=starts;const run=value as any;if(starts===1){run.status='failed';run.resources[0].releaseId=null;run.resources[0].requestPending='disk:static';run.events=[{at:new Date().toISOString(),level:'error',serviceId:'web',message:'HTTP 400: not_enough_storage_space [POST /v1/projects/test-web/disks]'}];}else{run.plan=recommendation;run.status='succeeded';run.events=[{at:new Date().toISOString(),level:'success',serviceId:'web',message:'SIMULATED upgrade and resume using existing resources.'}];}return run;}return value;
 }});
 await page.goto('/');await page.getByLabel('توکن API لیارا',{exact:true}).fill('test-token');await page.getByLabel('با نحوه استفاده از هوش مصنوعی برای تحلیل پروژه آشنا هستم.').check();await page.getByRole('button',{name:'اتصال حساب',exact:true}).click();await page.getByRole('button',{name:/پروژه شما از اینجا شروع می‌شود/}).click();await startPlanning(page,'fa');await page.getByLabel('SECRET_KEY',{exact:true}).fill('fixture');await page.getByRole('button',{name:'بررسی و استقرار',exact:true}).click();await page.getByRole('button',{name:'تأیید و استقرار',exact:true}).click();
 await expect(page.getByRole('dialog')).toContainText('فضای ذخیره‌سازی برنامه کافی نیست');await expect(page.getByRole('dialog')).toContainText('۷ گیگابایت');await expect(page.getByRole('dialog')).toContainText('۱۰ گیگابایت');await expect(page.getByRole('dialog')).toContainText('۲۴۰٬۰۰۰');
 expect(await page.evaluate(()=>(window as any).__DEPLOY_STARTS__)).toBe(1);
 await page.screenshot({path:'docs/screenshots/storage-recovery-fa.png',fullPage:true});
 await page.getByRole('dialog').getByRole('button',{name:'بررسی پیشنهاد و ادامه استقرار',exact:true}).click();await expect(page.getByRole('dialog')).toContainText('ارتقای برنامه موجود');expect(await page.evaluate(()=>(window as any).__DEPLOY_STARTS__)).toBe(1);expect(await page.evaluate(()=>(window as any).__RECOVERY_APPROVAL_ARGS__.resumeId)).toBe('run-test');
 await page.getByRole('dialog').getByRole('button',{name:'تأیید و ادامه',exact:true}).click();await expect(page.getByText('SIMULATED upgrade and resume using existing resources.')).toBeVisible();expect(await page.evaluate(()=>(window as any).__DEPLOY_STARTS__)).toBe(2);
});

test('original project changes require diff approval, cancellation does not apply, and a new cloud approval follows',async({page})=>{
 await mockNative(page);
 await page.addInitScript(()=>{const original=window.__LIARA_TEST_INVOKE__!;(window as any).__FIX_APPLIES__=0;(window as any).__STARTS__=0;window.__LIARA_TEST_INVOKE__=async(command,args)=>{
  if(command==='propose_project_fix')return {id:'fix-review',planId:'plan-test',revision:'revision-test',explanation:'The startup code needs a source edit.',changes:[{path:'app.py',before:'print("old")',after:'print("fixed")',reason:'Use the corrected startup path.'}],expiresAt:new Date(Date.now()+900000).toISOString()};
  if(command==='apply_project_fix'){(window as any).__FIX_APPLIES__++;return original('scan_project',{});}
  if(command==='plan_environment')return {web:{SECRET_KEY:'fixture-value'}};
  const value=await original(command,args);
  if(command==='start_deployment'){(window as any).__STARTS__++;const run=value as any;run.status='failed';run.events=[{at:new Date().toISOString(),level:'error',serviceId:'web',message:'test-web release FAILED: invalid startup code'}];return run;}
  return value;
 }});
 await page.goto('/');await connect(page);await page.getByRole('button',{name:/Your project starts here/}).click();await startPlanning(page);await page.getByRole('button',{name:'Review & deploy',exact:true}).click();await page.getByRole('button',{name:'Approve & deploy',exact:true}).click();
 await page.getByRole('dialog').getByRole('button',{name:'Review a project fix with the assistant'}).click();await expect(page.getByRole('dialog')).toContainText('Review proposed project changes');await expect(page.getByRole('dialog')).toContainText('app.py');await expect(page.locator('.diff-before')).toContainText('print("old")');await expect(page.locator('.diff-after')).toContainText('print("fixed")');expect(await page.evaluate(()=>(window as any).__FIX_APPLIES__)).toBe(0);
 await page.getByRole('dialog').getByRole('button',{name:'Go back',exact:true}).click();expect(await page.evaluate(()=>(window as any).__FIX_APPLIES__)).toBe(0);
 await page.getByRole('button',{name:'Ask the assistant',exact:true}).click();await page.getByRole('dialog').getByRole('button',{name:'Approve changes & prepare deployment',exact:true}).click();
 await expect(page.getByRole('dialog')).toContainText('Ready to put it online?');expect(await page.evaluate(()=>(window as any).__FIX_APPLIES__)).toBe(1);expect(await page.evaluate(()=>(window as any).__STARTS__)).toBe(1);
 const reuse=await page.evaluate(()=>(window as any).__PLANNING_CHOICES__);expect(reuse.services.web.existingId).toBe('test-id');
});

test('a successful recovery does not reopen an old failure modal',async({page})=>{
 await mockNative(page);
 await page.addInitScript(()=>{const original=window.__LIARA_TEST_INVOKE__!;window.__LIARA_TEST_INVOKE__=async(command,args)=>{
  const value=await original(command,args);if(command==='start_deployment'){const run=value as any;run.events.unshift({at:new Date(Date.now()-60000).toISOString(),level:'error',serviceId:'web',message:'HTTP 500: an earlier failed submission'});}return value;
 }});
 await page.goto('/');await connect(page);await page.getByRole('button',{name:/Your project starts here/}).click();await startPlanning(page);await page.getByLabel('SECRET_KEY',{exact:true}).fill('fixture');await page.getByRole('button',{name:'Review & deploy',exact:true}).click();await page.getByRole('button',{name:'Approve & deploy',exact:true}).click();await expect(page.getByRole('heading',{name:'Hello, world.'})).toBeVisible();await expect(page.getByRole('dialog')).toHaveCount(0);await expect(page.getByRole('alert')).toHaveCount(0);
});
