use boa_engine::{Context, Source};
use serde::{Deserialize, Serialize};

use crate::dom::{Dom, NodeKind};
use crate::storage::ScriptStorageSnapshot;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DomMutation {
    pub target_id: String,
    pub kind: String,
    pub value: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CanvasCommand {
    pub canvas_id: String,
    pub op: String,
    pub x: f32,
    pub y: f32,
    pub x2: f32,
    pub y2: f32,
    pub w: f32,
    pub h: f32,
    pub radius: f32,
    pub text: String,
    pub fill: String,
    pub stroke: String,
    pub font_size: f32,
    pub line_width: f32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScriptReport {
    pub discovered: usize,
    pub external_discovered: usize,
    pub executed: usize,
    pub blocked_external: usize,
    pub errors: usize,
    pub console: Vec<String>,
    pub title_override: Option<String>,
    pub body_html_override: Option<String>,
    pub last_error: Option<String>,
    pub dom_mutations: Vec<DomMutation>,
    pub canvas_commands: Vec<CanvasCommand>,
    pub cookie_writes: Vec<String>,
    pub storage: ScriptStorageSnapshot,
    pub dom_content_loaded_dispatched: bool,
    pub load_dispatched: bool,
    pub live_node_count: usize,
    pub event_listener_count: usize,
}

/// ECMAScript boundary for the Veil engine.
///
/// 0.5 upgrades the compatibility DOM from a collection of ID stubs into a
/// persistent object graph for the lifetime of a navigation. Elements have
/// parent/child relationships, attributes and stable object identity. Event
/// dispatch implements capture -> target -> bubble ordering, preventDefault,
/// stopPropagation and once listeners. Mutations are recorded and replayed
/// into the Rust DOM before layout/paint.
#[derive(Default)]
pub struct JavascriptSandbox;

/// Long-lived JavaScript runtime used by Veil Engine 0.8 for post-paint
/// interactions. The runtime stays inside the per-site engine process; it is
/// never shared across threads because Boa's Context is intentionally !Send.
pub struct LiveJavascriptRuntime {
    context: Context,
    discovered: usize,
    external_discovered: usize,
    executed: usize,
    blocked_external: usize,
    errors: usize,
    last_error: Option<String>,
}

impl JavascriptSandbox {
    pub fn run(
        &self,
        dom: &Dom,
        enabled: bool,
        external_sources: &[String],
        external_discovered: usize,
        storage: &ScriptStorageSnapshot,
    ) -> ScriptReport {
        if !enabled {
            return ScriptReport {
                external_discovered,
                discovered: dom
                    .nodes
                    .iter()
                    .enumerate()
                    .filter(|(_, node)| matches!(&node.kind, NodeKind::Element(el) if el.tag == "script" && !el.attrs.contains_key("src") && is_executable_script(el.attrs.get("type").map(String::as_str))))
                    .count()
                    + external_discovered,
                blocked_external: external_discovered,
                storage: storage.clone(),
                ..ScriptReport::default()
            };
        }
        let (_, report) = LiveJavascriptRuntime::new(dom, external_sources, external_discovered, storage);
        report
    }
}

impl LiveJavascriptRuntime {
    pub fn new(
        dom: &Dom,
        external_sources: &[String],
        external_discovered: usize,
        storage: &ScriptStorageSnapshot,
    ) -> (Self, ScriptReport) {
        let inline_sources: Vec<String> = dom
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(idx, node)| {
                let NodeKind::Element(el) = &node.kind else { return None; };
                if el.tag != "script" || el.attrs.contains_key("src") || !is_executable_script(el.attrs.get("type").map(String::as_str)) {
                    return None;
                }
                Some(dom.text_content(idx))
            })
            .collect();

        let mut context = Context::default();
        {
            let limits = context.runtime_limits_mut();
            limits.set_loop_iteration_limit(600_000);
            limits.set_recursion_limit(192);
            limits.set_stack_size_limit(2048);
        }

        let local_json = serde_json::to_string(&storage.local).unwrap_or_else(|_| "{}".into());
        let session_json = serde_json::to_string(&storage.session).unwrap_or_else(|_| "{}".into());
        let cookie_json = serde_json::to_string(&storage.cookie).unwrap_or_else(|_| "\"\"".into());
        let bootstrap = format!(
            "globalThis.__vvInitialLocal={local_json};globalThis.__vvInitialSession={session_json};globalThis.__vvInitialCookie={cookie_json};\n{JS_PRELUDE}\n{}",
            build_dom_registration(dom)
        );

        let mut runtime = Self {
            context,
            discovered: inline_sources.len() + external_discovered,
            external_discovered,
            executed: 0,
            blocked_external: 0,
            errors: 0,
            last_error: None,
        };

        if let Err(err) = runtime.context.eval(Source::from_bytes(bootstrap.as_str())) {
            runtime.errors += 1;
            runtime.last_error = Some(format!("JS bootstrap failed: {err}"));
            let report = runtime.snapshot(storage);
            return (runtime, report);
        }

        for source in inline_sources.iter().chain(external_sources.iter()) {
            if source.len() > 2_000_000 {
                runtime.errors += 1;
                runtime.last_error = Some("Script skipped because it exceeded the per-script safety limit.".into());
                continue;
            }
            match runtime.context.eval(Source::from_bytes(source.as_str())) {
                Ok(_) => runtime.executed += 1,
                Err(err) => {
                    runtime.errors += 1;
                    runtime.last_error = Some(format!("{err}"));
                }
            }
        }

        if let Err(err) = runtime.context.eval(Source::from_bytes(
            "__vvDispatch(document,'DOMContentLoaded');__vvDOMContentLoaded=true;__vvDispatch(globalThis,'load');__vvLoaded=true;"
        )) {
            runtime.errors += 1;
            runtime.last_error = Some(format!("Event bootstrap error: {err}"));
        }
        let _ = runtime.context.run_jobs();
        let report = runtime.snapshot(storage);
        (runtime, report)
    }

    pub fn dispatch_event(
        &mut self,
        node_id: usize,
        event_type: &str,
        value: Option<&str>,
        storage: &ScriptStorageSnapshot,
    ) -> (bool, ScriptReport) {
        let key = serde_json::to_string(&format!("n{node_id}")).unwrap_or_else(|_| "\"\"".into());
        let event = serde_json::to_string(event_type).unwrap_or_else(|_| "\"event\"".into());
        let value_script = value
            .map(|value| serde_json::to_string(value).unwrap_or_else(|_| "\"\"".into()))
            .map(|value| format!("node.value={value};"))
            .unwrap_or_default();
        let script = format!(
            "(()=>{{const node=__vvNodeByKey.get({key});if(!node)return true;{value_script}return node.dispatchEvent({{type:{event},bubbles:true,cancelable:true,target:node}});}})()"
        );
        let default_prevented = match self.context.eval(Source::from_bytes(script.as_str())) {
            Ok(value) => !value.to_boolean(),
            Err(err) => {
                self.errors += 1;
                self.last_error = Some(format!("Interaction event error: {err}"));
                false
            }
        };
        let _ = self.context.run_jobs();
        (default_prevented, self.snapshot(storage))
    }

    pub fn tick(&mut self, _elapsed_ms: u64, storage: &ScriptStorageSnapshot) -> ScriptReport {
        if let Err(err) = self.context.eval(Source::from_bytes("__vvRunTimers();")) {
            self.errors += 1;
            self.last_error = Some(format!("Timer event error: {err}"));
        }
        let _ = self.context.run_jobs();
        self.snapshot(storage)
    }

    fn snapshot(&mut self, fallback_storage: &ScriptStorageSnapshot) -> ScriptReport {
        let mut report = ScriptReport::default();
        report.discovered = self.discovered;
        report.external_discovered = self.external_discovered;
        report.executed = self.executed;
        report.blocked_external = self.blocked_external;
        report.errors = self.errors;
        report.last_error = self.last_error.clone();
        report.storage = fallback_storage.clone();
        report.dom_content_loaded_dispatched = eval_bool(&mut self.context, "!!__vvDOMContentLoaded").unwrap_or(false);
        report.load_dispatched = eval_bool(&mut self.context, "!!__vvLoaded").unwrap_or(false);
        report.title_override = eval_string(&mut self.context, "String(document.title || '')").filter(|value| !value.trim().is_empty());
        report.body_html_override = eval_string(&mut self.context, "String(document.body && document.body.__vvExplicitInnerHTML || '')").filter(|value| !value.trim().is_empty());
        if let Some(console) = eval_string(&mut self.context, "__vvConsole.join('\\n')") {
            report.console = console.lines().take(120).map(str::to_owned).collect();
        }
        report.dom_mutations = eval_json(&mut self.context, "JSON.stringify(__vvMutations)").unwrap_or_default();
        report.canvas_commands = eval_json(&mut self.context, "JSON.stringify(__vvCanvasCommands)").unwrap_or_default();
        report.cookie_writes = eval_json(&mut self.context, "JSON.stringify(__vvCookieWrites)").unwrap_or_default();
        report.storage.local = eval_json(&mut self.context, "JSON.stringify(localStorage.__dump())").unwrap_or_default();
        report.storage.session = eval_json(&mut self.context, "JSON.stringify(sessionStorage.__dump())").unwrap_or_default();
        report.live_node_count = eval_usize(&mut self.context, "__vvAllNodes.length").unwrap_or_default();
        report.event_listener_count = eval_usize(&mut self.context, "__vvListenerCount").unwrap_or_default();
        report
    }
}

fn build_dom_registration(dom: &Dom) -> String {
    let mut out = String::new();
    for (idx, node) in dom.nodes.iter().enumerate() {
        let NodeKind::Element(el) = &node.kind else { continue; };
        if el.tag == "document" { continue; }
        let key_json = serde_json::to_string(&format!("n{idx}")).unwrap();
        let id_json = serde_json::to_string(el.attrs.get("id").map(String::as_str).unwrap_or("")).unwrap();
        let tag_json = serde_json::to_string(&el.tag).unwrap_or_else(|_| "\"div\"".into());
        let text_json = serde_json::to_string(&dom.text_content(idx)).unwrap_or_else(|_| "\"\"".into());
        let attrs_json = serde_json::to_string(&el.attrs).unwrap_or_else(|_| "{}".into());
        let parent_json = nearest_element_parent(dom, idx)
            .map(|parent| serde_json::to_string(&format!("n{parent}")).unwrap())
            .unwrap_or_else(|| "null".into());
        out.push_str(&format!("__vvRegisterElement({key_json},{id_json},{tag_json},{text_json},{attrs_json},{parent_json});\n"));
    }
    let title = dom.find_first_tag("title").map(|idx| dom.text_content(idx)).unwrap_or_default();
    let title_json = serde_json::to_string(&title).unwrap_or_else(|_| "\"\"".into());
    out.push_str(&format!("__vvFinalizeDom();document.title={title_json};\n"));
    out
}

fn nearest_element_parent(dom: &Dom, idx: usize) -> Option<usize> {
    let mut parent = dom.nodes.get(idx)?.parent;
    while let Some(candidate) = parent {
        match &dom.nodes[candidate].kind {
            NodeKind::Element(el) if el.tag != "document" => return Some(candidate),
            _ => parent = dom.nodes[candidate].parent,
        }
    }
    None
}

fn eval_string(context: &mut Context, source: &str) -> Option<String> {
    let value = context.eval(Source::from_bytes(source)).ok()?;
    value.to_string(context).ok().map(|text| text.to_std_string_escaped())
}

fn eval_bool(context: &mut Context, source: &str) -> Option<bool> {
    let value = context.eval(Source::from_bytes(source)).ok()?;
    Some(value.to_boolean())
}

fn eval_usize(context: &mut Context, source: &str) -> Option<usize> {
    let value = context.eval(Source::from_bytes(source)).ok()?;
    value.to_number(context).ok().map(|number| number.max(0.0) as usize)
}

fn eval_json<T: for<'de> Deserialize<'de>>(context: &mut Context, source: &str) -> Option<T> {
    let text = eval_string(context, source)?;
    serde_json::from_str(&text).ok()
}

fn is_executable_script(kind: Option<&str>) -> bool {
    let Some(kind) = kind.map(str::trim).filter(|value| !value.is_empty()) else { return true; };
    matches!(kind.to_ascii_lowercase().as_str(),
        "text/javascript" | "application/javascript" | "application/ecmascript" | "text/ecmascript" | "module")
}

const JS_PRELUDE: &str = r#"
'use strict';
globalThis.window = globalThis;
globalThis.self = globalThis;
globalThis.__vvConsole = [];
globalThis.__vvMutations = [];
globalThis.__vvCanvasCommands = [];
globalThis.__vvCookieWrites = [];
globalThis.__vvTimers = [];
globalThis.__vvDOMContentLoaded = false;
globalThis.__vvLoaded = false;
globalThis.__vvListenerCount = 0;

function __vvMakeEvent(input, target) {
  const evt = input && typeof input === 'object' ? input : {type:String(input)};
  evt.type = String(evt.type || '');
  evt.target = evt.target || target;
  evt.currentTarget = null;
  evt.eventPhase = 0;
  evt.bubbles = evt.bubbles !== false;
  evt.cancelable = evt.cancelable !== false;
  evt.defaultPrevented = !!evt.defaultPrevented;
  evt.__stopped = false;
  evt.__immediate = false;
  evt.preventDefault = function(){ if(this.cancelable) this.defaultPrevented=true; };
  evt.stopPropagation = function(){ this.__stopped=true; };
  evt.stopImmediatePropagation = function(){ this.__stopped=true; this.__immediate=true; };
  return evt;
}
function __vvInvoke(target, evt, capture, phase) {
  const list = target && target.__vvListeners ? [...(target.__vvListeners[evt.type] || [])] : [];
  evt.currentTarget = target; evt.eventPhase = phase;
  for (const item of list) {
    if (!!item.capture !== !!capture) continue;
    try { item.fn.call(target, evt); } catch(e) { __vvConsole.push('event error: '+e); }
    if (item.once) target.removeEventListener(evt.type, item.fn, {capture:item.capture});
    if (evt.__immediate) break;
  }
}
function __vvEventTarget(obj) {
  Object.defineProperty(obj,'__vvListeners',{value:Object.create(null),enumerable:false,configurable:false});
  obj.addEventListener=function(type,fn,options=false){
    if(typeof fn!=='function') return;
    type=String(type); const capture=typeof options==='boolean'?options:!!(options&&options.capture); const once=!!(options&&typeof options==='object'&&options.once);
    const list=(this.__vvListeners[type] ||= []);
    if(!list.some(item=>item.fn===fn&&item.capture===capture)){list.push({fn,capture,once});__vvListenerCount++;}
  };
  obj.removeEventListener=function(type,fn,options=false){
    const capture=typeof options==='boolean'?options:!!(options&&options.capture); const list=this.__vvListeners[String(type)]||[];
    const i=list.findIndex(item=>item.fn===fn&&item.capture===capture); if(i>=0){list.splice(i,1);__vvListenerCount=Math.max(0,__vvListenerCount-1);}
  };
  obj.dispatchEvent=function(input){
    const evt=__vvMakeEvent(input,this); const path=[]; let p=this.parentNode;
    while(p){path.push(p);p=p.parentNode;}
    for(let i=path.length-1;i>=0&&!evt.__stopped;i--) __vvInvoke(path[i],evt,true,1);
    if(!evt.__stopped){__vvInvoke(this,evt,true,2);if(!evt.__immediate)__vvInvoke(this,evt,false,2);}
    if(evt.bubbles&&!evt.__stopped){for(let i=0;i<path.length&&!evt.__stopped;i++)__vvInvoke(path[i],evt,false,3);}
    evt.eventPhase=0;evt.currentTarget=null;return !evt.defaultPrevented;
  };
  return obj;
}
function __vvDispatch(target,type){if(target&&target.dispatchEvent)target.dispatchEvent({type});}
__vvEventTarget(globalThis);

globalThis.console=Object.freeze({
  log:(...a)=>__vvConsole.push(a.map(String).join(' ')), info:(...a)=>__vvConsole.push(a.map(String).join(' ')),
  warn:(...a)=>__vvConsole.push(a.map(String).join(' ')), error:(...a)=>__vvConsole.push(a.map(String).join(' '))
});

const __vvNodeByKey=new Map(), __vvNodeById=new Map();
globalThis.__vvAllNodes=[];
let __vvSyntheticId=0;
function __vvMutation(node,kind,value){__vvMutations.push({target_id:node&&node.__vvKey?('@node:'+node.__vvKey):'__body__',kind,value:String(value??'')});}
function __vvStyle(node){
  const raw=Object.create(null);
  return new Proxy(raw,{set(target,prop,value){target[prop]=String(value);__vvMutation(node,'style-set',String(prop)+':'+String(value));return true;}});
}
function __vvSerializeNode(node){
  if(!node) return '';
  if(node.nodeType===3) return String(node.data||'');
  const tag=String(node.tagName||'div').toLowerCase(); let attrs='';
  for(const [k,v] of Object.entries(node.attributes||{})){attrs+=' '+k+'="'+String(v).replace(/"/g,'&quot;')+'"';}
  const content=node.__vvExplicitInnerHTML || node.children.map(__vvSerializeNode).join('') || node.__vvText || '';
  return '<'+tag+attrs+'>'+content+'</'+tag+'>';
}
function __vvClassList(node){return {
  contains(c){return String(node.className||'').split(/\s+/).includes(String(c));},
  add(...items){const s=new Set(String(node.className||'').split(/\s+/).filter(Boolean));for(const i of items)s.add(String(i));node.className=[...s].join(' ');node.setAttribute('class',node.className);},
  remove(...items){const del=new Set(items.map(String));node.className=String(node.className||'').split(/\s+/).filter(x=>x&&!del.has(x)).join(' ');node.setAttribute('class',node.className);},
  toggle(c){c=String(c);if(this.contains(c)){this.remove(c);return false;}this.add(c);return true;}
};}
function __vvMatches(node,selector){
  selector=String(selector).trim();if(!selector||!node||node.nodeType!==1)return false;
  if(selector.startsWith('#'))return node.id===selector.slice(1);
  if(selector.startsWith('.'))return node.classList.contains(selector.slice(1));
  if(selector.startsWith('[')&&selector.endsWith(']'))return node.hasAttribute(selector.slice(1,-1).split('=')[0].trim());
  return node.tagName===selector.toUpperCase();
}
function __vvNewElement(key,id,tag,initialText,attrs){
  const node=__vvEventTarget({__vvKey:String(key),nodeType:1,tagName:String(tag).toUpperCase(),parentNode:null,children:[],attributes:Object.assign(Object.create(null),attrs||{}),style:null});
  node.style=__vvStyle(node);node.id=String(id||node.attributes.id||'');node.className=String(node.attributes.class||'');node.classList=__vvClassList(node);node.__vvText=String(initialText||'');node.__vvExplicitInnerHTML='';
  Object.defineProperty(node,'parentElement',{get(){return this.parentNode&&this.parentNode.nodeType===1?this.parentNode:null;}});
  Object.defineProperty(node,'firstChild',{get(){return this.children[0]||null;}});
  Object.defineProperty(node,'lastChild',{get(){return this.children[this.children.length-1]||null;}});
  Object.defineProperty(node,'textContent',{get(){return this.__vvText;},set(v){this.__vvText=String(v);this.__vvExplicitInnerHTML='';__vvMutation(this,'text',this.__vvText);}});
  Object.defineProperty(node,'innerHTML',{get(){return this.__vvExplicitInnerHTML||this.children.map(__vvSerializeNode).join('');},set(v){this.__vvExplicitInnerHTML=String(v);this.children=[];__vvMutation(this,'html',this.__vvExplicitInnerHTML);}});
  node.setAttribute=function(name,value){name=String(name).toLowerCase();value=String(value);this.attributes[name]=value;if(name==='id'){if(this.id)__vvNodeById.delete(this.id);this.id=value;if(value)__vvNodeById.set(value,this);}if(name==='class')this.className=value;__vvMutation(this,'attr-set',name+'\u0000'+value);};
  node.getAttribute=function(name){name=String(name).toLowerCase();return Object.prototype.hasOwnProperty.call(this.attributes,name)?this.attributes[name]:null;};
  node.hasAttribute=function(name){return Object.prototype.hasOwnProperty.call(this.attributes,String(name).toLowerCase());};
  node.removeAttribute=function(name){name=String(name).toLowerCase();delete this.attributes[name];if(name==='id'&&this.id){__vvNodeById.delete(this.id);this.id='';}if(name==='class')this.className='';__vvMutation(this,'attr-remove',name);};
  node.appendChild=function(child){if(!child)return child;if(child.parentNode)child.parentNode.removeChild(child);child.parentNode=this;this.children.push(child);__vvMutation(this,'append-html',__vvSerializeNode(child));return child;};
  node.prepend=function(child){if(!child)return;if(child.parentNode)child.parentNode.removeChild(child);child.parentNode=this;this.children.unshift(child);__vvMutation(this,'prepend-html',__vvSerializeNode(child));};
  node.removeChild=function(child){const i=this.children.indexOf(child);if(i>=0){this.children.splice(i,1);child.parentNode=null;__vvMutation(child,'remove','');}return child;};
  node.remove=function(){if(this.parentNode)this.parentNode.removeChild(this);else __vvMutation(this,'remove','');};
  node.insertAdjacentHTML=function(position,html){if(String(position).toLowerCase()==='afterbegin')__vvMutation(this,'prepend-html',String(html));else __vvMutation(this,'append-html',String(html));};
  node.matches=function(sel){return __vvMatches(this,sel);};
  node.querySelectorAll=function(sel){const out=[];const walk=n=>{for(const c of n.children||[]){if(__vvMatches(c,sel))out.push(c);walk(c);}};walk(this);return out;};
  node.querySelector=function(sel){return this.querySelectorAll(sel)[0]||null;};
  node.click=function(){return this.dispatchEvent({type:'click',bubbles:true,cancelable:true,target:this});};
  if(node.tagName==='CANVAS'){const ctx=__vvCanvasContext(node.id||node.__vvKey);node.getContext=kind=>String(kind).toLowerCase()==='2d'?ctx:null;}
  __vvNodeByKey.set(node.__vvKey,node);if(node.id)__vvNodeById.set(node.id,node);__vvAllNodes.push(node);return node;
}
function __vvRegisterElement(key,id,tag,initialText,attrs,parentKey){
  const node=__vvNewElement(key,id,tag,initialText,attrs);node.__vvParentKey=parentKey;return node;
}
function __vvFinalizeDom(){
  for(const node of __vvAllNodes){if(node.__vvParentKey){const p=__vvNodeByKey.get(node.__vvParentKey);if(p){node.parentNode=p;p.children.push(node);}}}
  document.documentElement=__vvAllNodes.find(n=>n.tagName==='HTML')||null;
  document.body=__vvAllNodes.find(n=>n.tagName==='BODY')||__vvNewElement('synthetic-body','','body','',{},null);
  if(document.documentElement&&!document.documentElement.parentNode){document.documentElement.parentNode=document;document.children=[document.documentElement];}
  else if(document.body&&!document.body.parentNode){document.body.parentNode=document;document.children=[document.body];}
}

function __vvCanvasContext(id){
  let fillStyle='#000000',strokeStyle='#000000',font='16px sans-serif',lineWidth=1,path=[];
  function fontSize(){const m=/([0-9.]+)px/.exec(font);return m?Number(m[1]):16;}
  function cmd(op,o={}){__vvCanvasCommands.push(Object.assign({canvas_id:id,op,x:0,y:0,x2:0,y2:0,w:0,h:0,radius:0,text:'',fill:fillStyle,stroke:strokeStyle,font_size:fontSize(),line_width:lineWidth},o));}
  return {
    get fillStyle(){return fillStyle;},set fillStyle(v){fillStyle=String(v);},get strokeStyle(){return strokeStyle;},set strokeStyle(v){strokeStyle=String(v);},
    get font(){return font;},set font(v){font=String(v);},get lineWidth(){return lineWidth;},set lineWidth(v){lineWidth=Math.max(.1,+v||1);},
    fillRect(x,y,w,h){cmd('fillRect',{x:+x||0,y:+y||0,w:+w||0,h:+h||0});},clearRect(x,y,w,h){cmd('clearRect',{x:+x||0,y:+y||0,w:+w||0,h:+h||0});},
    strokeRect(x,y,w,h){cmd('strokeRect',{x:+x||0,y:+y||0,w:+w||0,h:+h||0});},fillText(text,x,y){cmd('fillText',{text:String(text),x:+x||0,y:+y||0});},
    strokeText(text,x,y){cmd('strokeText',{text:String(text),x:+x||0,y:+y||0});},measureText(text){return {width:String(text).length*fontSize()*.55};},
    beginPath(){path=[];},closePath(){if(path.length>1)path.push(Object.assign({},path[0]));},moveTo(x,y){path.push({kind:'point',x:+x||0,y:+y||0,move:true});},
    lineTo(x,y){path.push({kind:'point',x:+x||0,y:+y||0,move:false});},arc(x,y,r,_s,_e){path.push({kind:'arc',x:+x||0,y:+y||0,r:Math.max(0,+r||0)});},
    stroke(){let prev=null;for(const p of path){if(p.kind==='arc'){cmd('strokeArc',{x:p.x,y:p.y,radius:p.r});prev=null;}else if(p.move||!prev){prev=p;}else{cmd('line',{x:prev.x,y:prev.y,x2:p.x,y2:p.y});prev=p;}}},
    fill(){for(const p of path)if(p.kind==='arc')cmd('fillArc',{x:p.x,y:p.y,radius:p.r});},save(){},restore(){},translate(){},scale(){},rotate(){}
  };
}

globalThis.document=__vvEventTarget({nodeType:9,parentNode:null,children:[],title:'',documentElement:null,body:null});
document.createElement=tag=>__vvNewElement('s'+(++__vvSyntheticId),'',String(tag),'',{},null);
document.createTextNode=text=>({__vvKey:'t'+(++__vvSyntheticId),nodeType:3,data:String(text),textContent:String(text),parentNode:null});
document.getElementById=id=>__vvNodeById.get(String(id))||null;
document.querySelectorAll=selector=>__vvAllNodes.filter(node=>__vvMatches(node,selector));
document.querySelector=selector=>document.querySelectorAll(selector)[0]||null;
document.write=(...args)=>{if(document.body)document.body.innerHTML=document.body.innerHTML+args.map(String).join('');};
document.writeln=(...args)=>document.write(...args,'\n');
let __vvCookie=String(__vvInitialCookie||'');
Object.defineProperty(document,'cookie',{get(){return __vvCookie;},set(v){v=String(v);__vvCookieWrites.push(v);const first=v.split(';')[0];const eq=first.indexOf('=');if(eq>0){const name=first.slice(0,eq).trim();const value=first.slice(eq+1).trim();const parts=__vvCookie.split(';').map(x=>x.trim()).filter(Boolean).filter(x=>!x.startsWith(name+'='));parts.push(name+'='+value);__vvCookie=parts.join('; ');}}});

globalThis.navigator=Object.freeze({userAgent:'Mozilla/5.0 (Veil; privacy) VeilBrowser/0.8.0 VeilEngine/0.8.0',language:'en-US',languages:Object.freeze(['en-US','en']),doNotTrack:'1',globalPrivacyControl:true,hardwareConcurrency:4});
function __vvStorage(initial){const data=Object.assign(Object.create(null),initial||{});return Object.freeze({getItem(k){k=String(k);return Object.prototype.hasOwnProperty.call(data,k)?data[k]:null;},setItem(k,v){data[String(k)]=String(v);},removeItem(k){delete data[String(k)];},clear(){for(const k of Object.keys(data))delete data[k];},key(i){return Object.keys(data)[Number(i)]??null;},get length(){return Object.keys(data).length;},__dump(){return Object.assign({},data);}});}
globalThis.localStorage=__vvStorage(__vvInitialLocal);globalThis.sessionStorage=__vvStorage(__vvInitialSession);
globalThis.setTimeout=(fn,_ms=0,...args)=>{if(typeof fn==='function'&&__vvTimers.length<192){__vvTimers.push(()=>fn(...args));return __vvTimers.length;}return 0;};
globalThis.clearTimeout=_id=>{};globalThis.requestAnimationFrame=fn=>setTimeout(()=>fn(0),0);globalThis.cancelAnimationFrame=clearTimeout;
globalThis.__vvRunTimers=()=>{let guard=0;while(__vvTimers.length&&guard++<192){const fn=__vvTimers.shift();try{fn();}catch(e){__vvConsole.push('timer error: '+e);}}};

globalThis.fetch=undefined;globalThis.XMLHttpRequest=undefined;globalThis.WebSocket=undefined;globalThis.EventSource=undefined;globalThis.Worker=undefined;globalThis.SharedWorker=undefined;globalThis.WebAssembly=undefined;
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vm_runs_dom_events_storage_and_canvas() {
        let dom = Dom::parse("<canvas id='c'></canvas><p id='msg'>Old</p><script>document.addEventListener('DOMContentLoaded',()=>{document.getElementById('msg').textContent='New';localStorage.setItem('k','v');let c=document.getElementById('c').getContext('2d');c.fillStyle='#ff0000';c.fillRect(1,2,3,4)});document.title='Safe';console.log('ok')</script>");
        let report = JavascriptSandbox::default().run(&dom, true, &[], 0, &ScriptStorageSnapshot::default());
        assert_eq!(report.title_override.as_deref(), Some("Safe"));
        assert_eq!(report.console, vec!["ok".to_owned()]);
        assert_eq!(report.storage.local.get("k").map(String::as_str), Some("v"));
        assert!(report.dom_mutations.iter().any(|mutation| mutation.value == "New"));
        assert!(report.canvas_commands.iter().any(|command| command.canvas_id == "c" && command.op == "fillRect"));
        assert!(report.dom_content_loaded_dispatched);
        assert!(report.live_node_count >= 2);
    }

    #[test]
    fn events_capture_then_bubble() {
        let dom = Dom::parse("<div id='outer'><button id='inner'>Go</button></div><script>let o=document.getElementById('outer'),i=document.getElementById('inner');o.addEventListener('click',()=>console.log('capture'),true);i.addEventListener('click',()=>console.log('target'));o.addEventListener('click',()=>console.log('bubble'));i.click()</script>");
        let report = JavascriptSandbox::default().run(&dom, true, &[], 0, &ScriptStorageSnapshot::default());
        assert_eq!(report.console, vec!["capture", "target", "bubble"]);
    }

    #[test]
    fn javascript_can_be_disabled_per_site() {
        let dom = Dom::parse("<script>document.title='Nope'</script>");
        let report = JavascriptSandbox::default().run(&dom, false, &[], 0, &ScriptStorageSnapshot::default());
        assert!(report.title_override.is_none());
        assert_eq!(report.discovered, 1);
    }
}
