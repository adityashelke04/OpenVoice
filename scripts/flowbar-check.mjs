/** Render and exercise the real flow bar in Chrome; all artifacts stay in target/. */
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { launchChrome, openPage, sleep } from './cdp.mjs';
const root = dirname(dirname(fileURLToPath(import.meta.url)));
const out = join(root, 'target', 'flow-review');
mkdirSync(out, {recursive:true});
const port = 9497;
const {chrome,browser} = await launchChrome({port,profile:join(root,'target','flow-review-browser'),args:['--in-process-gpu','--disable-gpu-sandbox']});
const {page} = await openPage(browser,port);
const errors=[];
page.on('Runtime.exceptionThrown', e=>errors.push(e.exceptionDetails.text));
const evaluate = async expression => {
 const result = await page.send('Runtime.evaluate',{expression,returnByValue:true,awaitPromise:true});
 if(result.exceptionDetails) throw new Error(result.exceptionDetails.text);
 return result.result.value;
};
const click = async state => evaluate(`document.querySelector('[data-state="${state}"]').click()`);
try {
 await page.send('Emulation.setDeviceMetricsOverride',{width:1200,height:1500,deviceScaleFactor:1,mobile:false});
 await page.send('Page.navigate',{url:'http://localhost:5199/?window=flowbar'});
 for(let i=0;i<100;i++){if(await evaluate('!!document.querySelector(".fbs-demo")'))break;await sleep(200);}
 if(!await evaluate('!!document.querySelector(".fbs-demo")'))throw new Error('Review surface failed to load');
 await evaluate('document.fonts.ready.then(()=>true)');
 await sleep(600);
 const captures=[];
 for(const plate of ['document','editor','photo','canvas']){
   const rect=await evaluate(`(()=>{const r=document.querySelector('.fbs-plate[data-plate="${plate}"]').getBoundingClientRect();return{x:r.x,y:r.y+scrollY,width:r.width,height:r.height,scale:1}})()`);
   const {data}=await page.send('Page.captureScreenshot',{format:'png',clip:rect,captureBeyondViewport:true});
   const path=join(out,`${plate}.png`);writeFileSync(path,Buffer.from(data,'base64'));captures.push(path);
 }
 const overflow=await evaluate(`(()=>{const failures=[];document.querySelectorAll('.fbs-row .flowbar').forEach((bar,i)=>{const r=bar.getBoundingClientRect();bar.querySelectorAll('button,.flowbar-wave,.flowbar-spinner,.flowbar-icon,.flowbar-msg').forEach(el=>{const c=el.getBoundingClientRect();if(c.width&&c.height&&(c.left<r.left-1||c.right>r.right+1||c.top<r.top-1||c.bottom>r.bottom+1))failures.push({i,element:el.className,bar:[r.width,r.height],child:[c.width,c.height]});});});return failures})()`);
 await click('ready');await sleep(150);
 const motion=await evaluate(`new Promise(resolve=>{const box=document.querySelector('.fbs-demo .overlay-hit');const frames=[];const start=performance.now();document.querySelector('[data-state="handsfree"]').click();function frame(t){const r=box.getBoundingClientRect();frames.push({t:t-start,w:r.width,cx:r.x+r.width/2});if(t-start<220)requestAnimationFrame(frame);else resolve(frames)}requestAnimationFrame(frame)})`);
 const controls=await evaluate(`(()=>{document.querySelector('.fbs-demo .flowbar-stop').click();return true})()`);
 await sleep(160);
 const stopped=await evaluate('document.querySelector(".fbs-demo").dataset.demoState === "working"');
 await click('handsfree');await sleep(150);
 await evaluate('document.querySelector(".fbs-demo .flowbar-cancel").click()');await sleep(150);
 const cancelled=await evaluate('document.querySelector(".fbs-demo").dataset.demoState === "ready"');
 await page.send('Emulation.setEmulatedMedia',{features:[{name:'prefers-reduced-motion',value:'reduce'}]});
 await click('handsfree');await sleep(100);
 const reduced=await evaluate(`(()=>{const box=document.querySelector('.fbs-demo .overlay-hit');return{transition:getComputedStyle(box).transitionDuration,voiceAnimation:getComputedStyle(box.querySelector('.wave-bar i')).animationName,width:box.getBoundingClientRect().width}})()`);
 const report={captures,overflow,motion,controls:controls&&stopped&&cancelled,reduced,errors};
 writeFileSync(join(out,'report.json'),JSON.stringify(report,null,2));
 console.log(JSON.stringify(report));
 if(overflow.length||errors.length||!report.controls||Math.abs(motion.at(-1).w-102.5)>1||reduced.voiceAnimation!=='none')process.exitCode=1;
} finally {page.close();browser.close();chrome.kill();}
