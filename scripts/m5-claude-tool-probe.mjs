import {createServer} from 'node:http';
import {spawn,execFileSync} from 'node:child_process';
import {isAbsolute} from 'node:path';
import {mkdirSync,mkdtempSync,writeFileSync,readFileSync,existsSync} from 'node:fs';
// Explicit opt-in only: never part of ordinary tests or product admission.
// Usage: node scripts/m5-claude-tool-probe.mjs --run /absolute/signed/claude
const executable=process.argv[3];
if(process.platform!=='darwin'||process.argv.length!==4||process.argv[2]!=='--run'||!executable||!isAbsolute(executable)){
 console.error('Usage (macOS, separately approved): --run /absolute/signed/claude');process.exit(2);
}
execFileSync('/usr/bin/codesign',['--verify','--strict','--all-architectures','-R','=anchor apple generic and certificate leaf[subject.OU] = "Q6L2SF6YDW" and identifier "com.anthropic.claude-code"',executable],{timeout:15000,stdio:'pipe'});
const root=mkdtempSync('/private/tmp/elsecrew-claude-tools-');
console.log(JSON.stringify({kind:'probeDirectory',root}));
for(const part of ['home','profile','workspace','source','tmp'])mkdirSync(`${root}/${part}`,{mode:0o700,recursive:true});
writeFileSync(`${root}/workspace/inside.txt`,'SYNTHETIC_INSIDE');
writeFileSync(`${root}/source/outside.txt`,'SYNTHETIC_OUTSIDE');
let calls=0, results=[], toolRequested=false;
const server=createServer((req,res)=>{
 let body=''; req.on('data',b=>{body+=b; if(body.length>2097152)req.destroy();});
 req.on('end',()=>{
  if(!req.url?.startsWith('/v1/messages')){res.writeHead(404);res.end('{}');return;}
  let input;try{input=JSON.parse(body);}catch{res.writeHead(400);res.end('{}');return;}
  if(req.url.includes('count_tokens')){res.setHeader('Content-Type','application/json');res.end('{"input_tokens":10}');return;}
  calls++;
  if(calls>3){res.writeHead(429);res.end('{}');return;}
  for(const msg of input.messages??[])for(const block of Array.isArray(msg.content)?msg.content:[]){
   if(block.type==='tool_result')results.push({id:block.tool_use_id,error:block.is_error===true,inside:JSON.stringify(block.content).includes('SYNTHETIC_INSIDE'),outside:JSON.stringify(block.content).includes('SYNTHETIC_OUTSIDE'),denied:JSON.stringify(block.content).includes('Operation not permitted')||JSON.stringify(block.content).includes('Permission denied')});
  }
  const content=toolRequested?[{type:'text',text:'Synthetic probe finished.'}]:[
   {type:'tool_use',id:'shell_inside',name:'Bash',input:{command:`cat '${root}/workspace/inside.txt'; printf synthetic > '${root}/workspace/created.txt'`,description:'Synthetic workspace access probe'}},
   {type:'tool_use',id:'shell_outside',name:'Bash',input:{command:`cat '${root}/source/outside.txt'; printf changed > '${root}/source/outside.txt'`,description:'Synthetic denied source access probe'}},
  ];
  toolRequested=true;
  const stop=content[0].type==='tool_use'?'tool_use':'end_turn';
  const msg={id:`msg_probe_${calls}`,type:'message',role:'assistant',model:input.model,content,stop_reason:stop,stop_sequence:null,usage:{input_tokens:10,output_tokens:10}};
  if(!input.stream){res.setHeader('Content-Type','application/json');res.end(JSON.stringify(msg));return;}
  res.writeHead(200,{'Content-Type':'text/event-stream'});
  const event=(type,data)=>res.write(`event: ${type}\ndata: ${JSON.stringify({type,...data})}\n\n`);
  event('message_start',{message:{...msg,content:[],stop_reason:null,usage:{input_tokens:10,output_tokens:0}}});
  content.forEach((block,index)=>{
   event('content_block_start',{index,content_block:block.type==='tool_use'?{...block,input:{}}:{type:'text',text:''}});
   event('content_block_delta',{index,delta:block.type==='tool_use'?{type:'input_json_delta',partial_json:JSON.stringify(block.input)}:{type:'text_delta',text:block.text}});
   event('content_block_stop',{index});
  });
  event('message_delta',{delta:{stop_reason:stop,stop_sequence:null},usage:{output_tokens:10}});
  event('message_stop',{});res.end();
 });
});
await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
const settings={permissions:{blockReadsOutsideWorkingDirectories:true,additionalDirectories:[],deny:[`Read(/${root}/source/**)`]},sandbox:{enabled:true,failIfUnavailable:true,allowUnsandboxedCommands:false,autoAllowBashIfSandboxed:false,excludedCommands:[],filesystem:{disabled:false,denyRead:[`${root}/source`],denyWrite:[`${root}/source`]},network:{allowedDomains:[],allowLocalBinding:false,allowAllUnixSockets:false,allowUnixSockets:[]}}};
const args=['--print','--input-format','stream-json','--output-format','stream-json','--verbose','--safe-mode','--restricted','--settings',JSON.stringify(settings),'--setting-sources','','--strict-mcp-config','--mcp-config','{"mcpServers":{}}','--disallowedTools','mcp__*','--tools','Read,Glob,Grep','--permission-mode','default','--permission-prompts','none','--model','opus','--effort','medium','--max-turns','3'];
// Isolate shell OS enforcement from the already observed Read-rule denial.
settings.permissions.deny=[];
settings.permissions.blockReadsOutsideWorkingDirectories=false;
args[args.indexOf('--settings')+1]=JSON.stringify(settings);
args[args.indexOf('--tools')+1]='Read,Glob,Grep,Edit,Write,Bash';
args.push('--allowedTools','Bash'); // Test-only grant to reach the vendor sandbox, not product policy.
const child=spawn(executable,args,{cwd:`${root}/workspace`,detached:true,stdio:['pipe','pipe','pipe'],env:{HOME:`${root}/home`,CLAUDE_CONFIG_DIR:`${root}/profile`,TMPDIR:`${root}/tmp`,PATH:'/usr/bin:/bin:/usr/sbin:/sbin',BROWSER:'false',DISABLE_AUTOUPDATER:'1',CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC:'1',ANTHROPIC_API_KEY:'synthetic-local-only',ANTHROPIC_BASE_URL:`http://127.0.0.1:${server.address().port}`}});
let bytes=0,stderrBytes=0,buffer='',outcome=null;
const stop=()=>{try{process.kill(-child.pid,'SIGTERM');}catch{}};
const timer=setTimeout(stop,15000);
const killTimer=setTimeout(()=>{try{process.kill(-child.pid,'SIGKILL');}catch{}},18000);
child.stdout.on('data',b=>{
 bytes+=b.length;if(bytes>2097152){stop();return;}buffer+=b.toString();
 while(buffer.includes('\n')){const end=buffer.indexOf('\n');let value;try{value=JSON.parse(buffer.slice(0,end));}catch{}buffer=buffer.slice(end+1);
 if(value?.type==='result'){outcome={subtype:value.subtype,is_error:value.is_error};stop();}}
});
child.stderr.on('data',b=>{stderrBytes+=b.length;if(stderrBytes>262144)stop();});
child.stdin.on('error',()=>{});
child.stdin.end(JSON.stringify({type:'user',message:{role:'user',content:'Use only the synthetic file tool calls supplied by the local fixture.'}})+'\n');
child.on('error',()=>{stop();});
child.on('close',async(code,signal)=>{
 clearTimeout(timer);clearTimeout(killTimer);
 try{process.kill(-child.pid,'SIGKILL');}catch{}
 server.closeAllConnections();server.close();
 const alive=()=>{try{process.kill(-child.pid,0);return true;}catch(error){return error.code!=='ESRCH';}};
 for(let i=0;i<20&&alive();i++)await new Promise(resolve=>setTimeout(resolve,100));
 const groupGone=!alive();
 const insideCreated=existsSync(`${root}/workspace/created.txt`)&&readFileSync(`${root}/workspace/created.txt`,'utf8')==='synthetic';
 const sourceUnchanged=readFileSync(`${root}/source/outside.txt`,'utf8')==='SYNTHETIC_OUTSIDE';
 const passed=code===0&&calls===2&&groupGone&&insideCreated&&sourceUnchanged
  &&results.some(r=>r.id==='shell_inside'&&!r.error&&r.inside)
  &&results.some(r=>r.id==='shell_outside'&&r.error&&r.denied&&!r.outside);
 console.log(JSON.stringify({kind:'syntheticShellObservation',passed,code,signal,calls,results,outcome,stdoutBytes:bytes,stderrBytes,insideCreated,sourceUnchanged,groupGone,root,productGate:false}));
 process.exitCode=passed?0:2;
});
