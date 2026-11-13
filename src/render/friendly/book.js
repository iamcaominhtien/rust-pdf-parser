// Book view: the article laid out in pages (CSS columns over the one article
// DOM) and turned like paper. A turn bends the page along a fold line (the
// perpendicular bisector between the corner's rest position and where it is
// now), shows the next page on the back of the folded part, shades the fold
// and casts a shadow on the page beneath; the corner can be dragged by hand.
// A soft paper sound (Web Audio, no file) follows the motion.
(function(){
  var doc=document,root=doc.documentElement,P=window.p2h||{},article=doc.getElementById('article'),layout=doc.querySelector('.layout');
  if(!article||!layout)return;
  var title=(doc.querySelector('.rh-title')||{}).textContent||doc.title;
  var reduced=matchMedia('(prefers-reduced-motion:reduce)');
  var DUR=720;            // a full turn, ms
  var view=null,stage,real,win,turn,repA,repB,layerA,flapBox,flap,flapIn,blank,shRev,shFront,shFlap,prevBtns,nextBtns,countEl,barEl,soundBtn;
  var g={},page=0,pages=1,per=2,step=1,heads=[],active=false,stale=true,busy=false,queued=0,observer=null;

  function el(tag,cls,html){var e=doc.createElement(tag);if(cls)e.className=cls;if(html)e.innerHTML=html;return e;}
  var ARROW_L='<svg viewBox="0 0 16 16" aria-hidden="true"><path d="M10 3L5 8l5 5" fill="none" stroke="currentColor" stroke-width="1.6"/></svg>';
  var ARROW_R='<svg viewBox="0 0 16 16" aria-hidden="true"><path d="M6 3l5 5-5 5" fill="none" stroke="currentColor" stroke-width="1.6"/></svg>';
  var SPEAKER='<svg viewBox="0 0 16 16" aria-hidden="true"><path d="M2.5 6h2.5l3.5-3v10L5 10H2.5z" fill="currentColor"/><path class="wave" d="M11 5.5a3.5 3.5 0 0 1 0 5M12.75 3.75a6 6 0 0 1 0 8.5" fill="none" stroke="currentColor" stroke-width="1.3"/></svg>';

  // ---------- Building the view ----------
  function build(){
    view=el('div','bk-view');view.tabIndex=-1;view.setAttribute('role','region');view.setAttribute('aria-label','Book view');view.hidden=true;
    var row=el('div','bk-row');stage=el('div','bk-stage');
    real=spread();stage.appendChild(real);win=real.querySelector('.bk-window');
    turn=el('div','bk-turn');turn.hidden=true;turn.setAttribute('aria-hidden','true');turn.inert=true;
    layerA=el('div','bk-layer');shRev=el('div','bk-shade');shFront=el('div','bk-shade');
    flapBox=el('div','bk-flapbox');flap=el('div','bk-flap');flapIn=el('div','bk-flapin');flapIn.style.cssText='position:absolute;left:0;top:0;transform-origin:0 0';
    blank=el('div','bk-blank');shFlap=el('div','bk-shade');
    flap.appendChild(flapIn);flap.appendChild(shFlap);flapBox.appendChild(flap);
    turn.appendChild(layerA);turn.appendChild(shRev);turn.appendChild(shFront);turn.appendChild(flapBox);stage.appendChild(turn);
    var prev=navBtn('prev','Previous page',ARROW_L),next=navBtn('next','Next page',ARROW_R);
    row.appendChild(prev);row.appendChild(stage);row.appendChild(next);
    var foot=el('div','bk-foot');
    countEl=el('span','bk-count');countEl.setAttribute('aria-live','polite');
    barEl=el('div','bk-bar','<i></i>');
    soundBtn=el('button','btn bk-sound',SPEAKER+'<span class="sr">Page sound</span>');soundBtn.type='button';soundBtn.setAttribute('aria-label','Page sound');
    soundBtn.setAttribute('aria-pressed',P.stored&&P.stored('p2h-sound')==='off'?'false':'true');
    soundBtn.addEventListener('click',function(){var on=soundBtn.getAttribute('aria-pressed')!=='true';soundBtn.setAttribute('aria-pressed',on?'true':'false');if(P.store)P.store('p2h-sound',on?'on':'off');});
    foot.appendChild(navBtn('prev','Previous page',ARROW_L));foot.appendChild(countEl);foot.appendChild(barEl);foot.appendChild(soundBtn);foot.appendChild(navBtn('next','Next page',ARROW_R));
    view.appendChild(row);view.appendChild(foot);doc.body.appendChild(view);
    prevBtns=view.querySelectorAll('.bk-nav.prev');nextBtns=view.querySelectorAll('.bk-nav.next');
    prevBtns.forEach(function(b){b.addEventListener('click',function(){go(-1,false);});});
    nextBtns.forEach(function(b){b.addEventListener('click',function(){go(1,false);});});
    view.addEventListener('keydown',onKey);
    stage.addEventListener('pointerdown',onDown);
  }
  function navBtn(kind,label,svg){var b=el('button','bk-nav '+kind,svg);b.type='button';b.setAttribute('aria-label',label);return b;}
  function spread(){
    var s=el('div','bk-spread');
    s.appendChild(el('div','bk-paper verso','<div class="bk-rh" aria-hidden="true"></div><div class="bk-folio" aria-hidden="true"></div>'));
    s.appendChild(el('div','bk-paper recto','<div class="bk-rh" aria-hidden="true"></div><div class="bk-folio" aria-hidden="true"></div>'));
    s.appendChild(el('div','bk-window'));
    return s;
  }

  // ---------- Geometry and pagination ----------
  function measure(){
    var vw=innerWidth,vh=innerHeight-48,two=vw>=1100&&innerWidth/innerHeight>=1.25,phone=vw<600,pw,ph,k;
    if(two){ph=Math.max(600,vh-72-32);pw=ph/Math.SQRT2;var maxW=(vw-2*(44+20)-48)/2;if(pw>maxW){pw=maxW;ph=pw*Math.SQRT2;}}
    else if(phone){pw=vw;ph=vh-56;}
    else{pw=Math.min(vw-48,640);ph=Math.min(vh-72-24,pw*Math.SQRT2);}
    pw=Math.floor(pw);ph=Math.floor(ph);
    k=Math.max(.6,Math.min(1.15,pw/560));
    var mt=phone?44:72*k,mb=phone?44:72*k,mi=phone?20:56*k,mo=phone?20:64*k;
    var fs=pw<420?15:16,lh=fs*1.6,th=Math.floor((ph-mt-mb)/lh)*lh,tw=Math.floor(pw-mi-mo);
    mb=ph-mt-th;
    g={two:two,phone:phone,pw:pw,ph:ph,mt:mt,mb:mb,mi:mi,mo:mo,tw:tw,th:th,fs:fs};
    per=two?2:1;
    var gap=two?2*mi:mi+mo;step=tw+gap;
    var vars={'--pw':pw,'--ph':ph,'--sw':two?2*pw:pw,'--rx':two?pw:0,'--mt':mt,'--mb':mb,'--mi':mi,'--mo':mo,'--tw':tw,'--th':th,'--gap':gap,
      '--winw':two?2*tw+gap:tw,'--wx':two?mo:mi};
    for(var name in vars)view.style.setProperty(name,vars[name]+'px');
    view.style.setProperty('--bk-fs',fs+'px');
    view.classList.toggle('bk-single',!two);
    real.querySelector('.verso').hidden=!two;
  }
  function paginate(){
    measure();
    var art=win.firstElementChild;art.style.setProperty('--bk-x','0px');
    pages=Math.max(1,Math.round((art.scrollWidth+(step-g.tw))/step));
    heads=[].map.call(art.querySelectorAll('h2,h3,h4'),function(h){
      var n=h.querySelector('.secno'),text=n?n.textContent+' '+h.textContent.slice(n.textContent.length):h.textContent;
      return {col:colOf(h),text:text.replace(/\s+/g,' ').trim(),el:h};});
    tocPages();
    stale=true;
  }
  function colOf(node){
    var art=win.firstElementChild,rects=node.getClientRects(),r=rects.length?rects[0]:node.getBoundingClientRect();
    var base=art.getBoundingClientRect().left;
    return Math.max(0,Math.floor((r.left-base+2)/step));
  }
  function tocPages(){
    var toc=doc.getElementById('toc');if(!toc)return;
    toc.querySelectorAll('a[href^="#"]').forEach(function(a){
      var t=doc.getElementById(a.getAttribute('href').slice(1)),pg=a.querySelector('.pg');
      if(!pg){pg=el('span','pg');a.appendChild(pg);}
      pg.textContent=t&&article.contains(t)?String(colOf(t)+1):'';
    });
  }
  function clearTocPages(){doc.querySelectorAll('nav.toc .pg').forEach(function(p){p.textContent='';});}
  function first(p){return Math.max(0,Math.min(p-(p%per),Math.floor((pages-1)/per)*per));}
  function headAt(p){var t='';for(var i=0;i<heads.length&&heads[i].col<=p;i++)t=heads[i].text;return t||title;}

  // Shows spread (or page) p in spread element s.
  function paint(s,p){
    s.querySelector('.bk-window>article').style.setProperty('--bk-x',(-p*step)+'px');
    var v=s.querySelector('.verso'),r=s.querySelector('.recto');
    if(per===2){
      v.querySelector('.bk-rh').textContent=title;v.querySelector('.bk-folio').textContent=String(p+1);
      r.querySelector('.bk-rh').textContent=p+1<pages?headAt(p+1):'';r.querySelector('.bk-folio').textContent=p+1<pages?String(p+2):'';
      r.style.visibility=p+1<pages?'':'hidden';
    }else{
      r.querySelector('.bk-rh').textContent=headAt(p);r.querySelector('.bk-folio').textContent=String(p+1);
    }
  }
  function show(p,announce){
    page=first(p);paint(real,page);
    var last=page+per>=pages,label=per===2&&page+1<pages?'Pages '+(page+1)+'–'+(page+2)+' of '+pages:'Page '+(page+1)+' of '+pages;
    countEl.textContent=label;barEl.firstChild.style.width=((Math.min(pages,page+per))/pages*100)+'%';
    var prog=doc.getElementById('progress');if(prog)prog.style.width=((Math.min(pages,page+per))/pages*100)+'%';
    prevBtns.forEach(function(b){b.disabled=page===0;});nextBtns.forEach(function(b){b.disabled=last;});
    win.scrollLeft=0;
    if(announce&&P.say)P.say(label);
  }
  function blockOnPage(p){
    var kids=win.firstElementChild.children;
    for(var i=0;i<kids.length;i++){var c=colOf(kids[i]);if(c>=p)return c<p+per?kids[i]:null;}
    return null;
  }

  // ---------- Replicas: clones of the spread used while a page turns ----------
  function replica(){
    var s=real.cloneNode(true);s.className='bk-spread bk-replica';
    s.querySelectorAll('[id]').forEach(function(n){n.removeAttribute('id');});
    s.querySelectorAll('img[loading]').forEach(function(n){n.removeAttribute('loading');});
    s.setAttribute('translate','no');return s;
  }
  function ensureReplicas(){
    if(!stale&&repA)return;
    if(repA)repA.remove();if(repB)repB.remove();
    repA=replica();repB=replica();layerA.appendChild(repA);flapIn.textContent='';flapIn.appendChild(repB);stale=false;
  }

  // ---------- The fold ----------
  // Sutherland-Hodgman: the part of convex polygon poly where (X-m)·n has sign s.
  function clipHalf(poly,m,n,s){
    var out=[],side=function(p){return s*((p[0]-m[0])*n[0]+(p[1]-m[1])*n[1]);};
    for(var i=0;i<poly.length;i++){
      var a=poly[i],b=poly[(i+1)%poly.length],da=side(a),db=side(b);
      if(da>=0)out.push(a);
      if((da>=0)!==(db>=0)){var t=da/(da-db);out.push([a[0]+(b[0]-a[0])*t,a[1]+(b[1]-a[1])*t]);}
    }
    return out;
  }
  function rect(x0,x1){return [[x0,0],[x1,0],[x1,g.ph],[x0,g.ph]];}
  function poly(p){return p.length?'polygon('+p.map(function(q){return q[0].toFixed(1)+'px '+q[1].toFixed(1)+'px';}).join(',')+')':'polygon(0 0,0 0,0 0)';}
  function path(polys){return 'path(\''+polys.filter(function(p){return p.length;}).map(function(p){return 'M'+p.map(function(q){return q[0].toFixed(1)+' '+q[1].toFixed(1);}).join('L')+'Z';}).join('')+'\')';}
  // A linear gradient on a stage-sized box, along unit vector u, starting at point m.
  function grad(u,m,stops){
    var w=g.two?2*g.pw:g.pw,h=g.ph,ang=Math.atan2(u[0],-u[1]),len=Math.abs(w*Math.sin(ang))+Math.abs(h*Math.cos(ang));
    var s0=(m[0]-w/2)*u[0]+(m[1]-h/2)*u[1]+len/2;
    return 'linear-gradient('+(ang*180/Math.PI).toFixed(2)+'deg,'+stops.map(function(st){return st[0]+' '+(s0+st[1]).toFixed(1)+'px';}).join(',')+')';
  }
  // Keeps the corner at its distance from both ends of the spine: paper does not stretch.
  function constrain(p){
    var sx=T.spine,ends=[[sx,0],[sx,g.ph]];
    for(var it=0;it<3;it++)ends.forEach(function(e){
      var max=Math.hypot(T.c[0]-e[0],T.c[1]-e[1]),dx=p[0]-e[0],dy=p[1]-e[1],d=Math.hypot(dx,dy);
      if(d>max){p=[e[0]+dx/d*max,e[1]+dy/d*max];}
    });
    return p;
  }
  var T=null;
  function begin(a,b,grabY){
    ensureReplicas();
    var y=Math.max(g.ph*0.55,Math.min(g.ph,grabY==null?g.ph*0.94:grabY));
    T={a:a,b:b,spine:per===2?g.pw:0,leaf:per===2?[g.pw,2*g.pw]:[0,g.pw]};
    T.c=[T.leaf[1],y];T.end=[2*T.spine-T.c[0],y];
    show(b,false);paint(repA,a);paint(repB,b);
    flapIn.textContent='';flapIn.appendChild(per===2?repB:blank);
    turn.hidden=false;layerA.style.opacity='';
  }
  function render(p){
    p=constrain(p);T.p=p;
    var cx=T.c[0]-p[0],cy=T.c[1]-p[1],d=Math.hypot(cx,cy),leaf=rect(T.leaf[0],T.leaf[1]),stat=per===2?rect(0,g.pw):[];
    if(d<0.5){layerA.style.clipPath=path([stat,leaf]);flap.style.clipPath=poly([]);shRev.style.background=shFront.style.background='none';return;}
    var n=[cx/d,cy/d],m=[(T.c[0]+p[0])/2,(T.c[1]+p[1])/2];
    var front=clipHalf(leaf,m,n,-1),corner=clipHalf(leaf,m,n,1);
    var refl=function(q){var k=2*((q[0]-m[0])*n[0]+(q[1]-m[1])*n[1]);return [q[0]-k*n[0],q[1]-k*n[1]];};
    var flapPoly=corner.map(refl);
    layerA.style.clipPath=path([stat,front]);
    flap.style.clipPath=poly(flapPoly);
    // Back of the leaf: the destination page turned by the composition of the two reflections (spine, fold).
    // Reflection across the fold (normal n): I - 2nnᵀ.
    var A11=1-2*n[0]*n[0],A12=-2*n[0]*n[1],A22=1-2*n[1]*n[1];
    var ux=2*T.spine-m[0],uy=-m[1];
    var e=m[0]+A11*ux+A12*uy,f=m[1]+A12*ux+A22*uy;
    flapIn.style.transform=per===2?'matrix('+(-A11)+','+(-A12)+','+A12+','+A22+','+e+','+f+')':'none';
    if(per!==2){blank.style.width=(2*g.pw)+'px';blank.style.left=(-g.pw)+'px';}
    var prog=Math.min(1,d/(2*(T.leaf[1]-T.leaf[0]))),depth=Math.sin(Math.PI*prog),len=Math.max(16,Math.min(d*0.32,g.pw*0.24));
    // The lifted leaf casts a soft shadow on the page it uncovers, darkest along the fold.
    shRev.style.clipPath=poly(corner);
    shRev.style.background=grad(n,m,[['rgba(0,0,0,'+(0.20*depth+0.06).toFixed(3)+')',0],['rgba(0,0,0,'+(0.08*depth).toFixed(3)+')',len*0.35],['rgba(0,0,0,0)',len]]);
    // The part still lying flat curves up towards the fold: a little darker there.
    shFront.style.clipPath=poly(front);
    shFront.style.background=grad([-n[0],-n[1]],m,[['rgba(0,0,0,'+(0.10*depth+0.02).toFixed(3)+')',0],['rgba(0,0,0,0)',Math.max(12,len*0.45)]]);
    // The back of the leaf: shade at the fold, light where the paper bulges, dimmer at the edge.
    var flapW=Math.max(1,d/2);
    shFlap.style.background=grad([-n[0],-n[1]],m,[['rgba(0,0,0,.22)',0],['rgba(0,0,0,.04)',Math.min(10,flapW*.18)],
      ['rgba(255,255,255,.16)',flapW*.38],['rgba(255,255,255,0)',flapW*.62],['rgba(0,0,0,.08)',flapW]]);
  }
  function end(complete){
    turn.hidden=true;T=null;busy=false;
    if(queued){var q=queued;queued=0;go(q,false);}
  }

  // ---------- Motion ----------
  function ease(t){return t<.5?4*t*t*t:1-Math.pow(-2*t+2,3)/2;}
  function animate(from,to,dur,lift,done){
    var t0=performance.now();
    (function frame(now){
      var t=Math.min(1,(now-t0)/dur),e=ease(t);
      render([from[0]+(to[0]-from[0])*e,from[1]+(to[1]-from[1])*e-lift*Math.sin(Math.PI*e)]);
      if(t<1)requestAnimationFrame(frame);else done();
    })(t0);
  }
  // Turns by dir spreads (±1); with focus, moves focus to the first block of the new page.
  function go(dir,focus){
    if(!active)return;
    if(busy){queued=Math.max(-1,Math.min(1,dir));return;}
    var target=first(page+dir*per);if(target===page)return;
    turnTo(target,focus,true);
  }
  function turnTo(target,focus,animated){
    var from=page,fwd=target>from;
    var near=Math.abs(target-from)===per;
    if(!animated||!near){show(target,true);afterTurn(focus);return;}
    busy=true;sound(DUR/1000,1);
    var a=fwd?from:target,b=fwd?target:from;
    begin(a,b,null);
    if(reduced.matches){
      // A short cross-fade from the old spread (replica) to the new one.
      paint(repA,from);show(target,false);
      layerA.style.clipPath='none';flap.style.clipPath=poly([]);shRev.style.background=shFront.style.background='none';
      layerA.style.opacity='1';layerA.style.transition='opacity 120ms linear';
      requestAnimationFrame(function(){requestAnimationFrame(function(){layerA.style.opacity='0';});});
      setTimeout(function(){layerA.style.transition='';show(target,true);afterTurn(focus);end(true);},150);
      return;
    }
    var start=fwd?T.c:T.end,stop=fwd?T.end:T.c;
    render(start);
    animate(start,stop,DUR,g.ph*0.12,function(){show(target,true);afterTurn(focus);end(true);});
  }
  function afterTurn(focus){
    if(!focus)return;
    var b=blockOnPage(page);if(!b)return;
    if(!b.hasAttribute('tabindex'))b.setAttribute('tabindex','-1');
    b.focus({preventScroll:true});win.scrollLeft=0;
  }

  // ---------- Pointer: drag the corner, click the edge, swipe ----------
  var drag=null;
  function onDown(e){
    if(!active||busy||e.button>0)return;
    if(e.target.closest('a,button,input,textarea,select,pre,.tbl-wrap,.eq-body,summary'))return;
    var r=stage.getBoundingClientRect(),x=e.clientX-r.left,y=e.clientY-r.top,w=r.width;
    var fwdZone=x>w*0.8,backZone=x<w*0.2;
    // The middle of the page is for reading and selecting text; touch can swipe there.
    if(!fwdZone&&!backZone&&e.pointerType!=='touch')return;
    drag={x0:e.clientX,y0:e.clientY,t0:performance.now(),moved:false,zone:fwdZone?1:backZone?-1:0,started:false,vx:0,lx:e.clientX,lt:performance.now(),id:e.pointerId,ry:y};
    stage.setPointerCapture(e.pointerId);
    stage.addEventListener('pointermove',onMove);stage.addEventListener('pointerup',onUp);stage.addEventListener('pointercancel',onUp);
  }
  function startDrag(){
    var dir=drag.zone,target=first(page+dir*per);
    if(target===page){drag.zone=0;return;}
    var fwd=dir>0;drag.fwd=fwd;drag.target=target;
    var a=fwd?page:target,b=fwd?target:page,from=page;
    begin(a,b,drag.ry);drag.from=from;drag.started=true;busy=true;soundStart();
    render(fwd?T.c:T.end);
  }
  function onMove(e){
    if(!drag)return;
    var dx=e.clientX-drag.x0,dy=e.clientY-drag.y0,now=performance.now();
    if(!drag.moved&&Math.hypot(dx,dy)>6)drag.moved=true;
    if(drag.moved&&drag.zone&&!drag.started)startDrag();
    var dt=Math.max(1,now-drag.lt);drag.vx=0.7*drag.vx+0.3*((e.clientX-drag.lx)/dt);drag.lx=e.clientX;drag.lt=now;
    if(drag.started){var r=stage.getBoundingClientRect();render([e.clientX-r.left,e.clientY-r.top]);soundSpeed(Math.abs(drag.vx));}
  }
  function onUp(e){
    stage.removeEventListener('pointermove',onMove);stage.removeEventListener('pointerup',onUp);stage.removeEventListener('pointercancel',onUp);
    var d=drag;drag=null;if(!d)return;
    var dx=e.clientX-d.x0,dy=e.clientY-d.y0;
    if(d.started){
      soundStop();
      var past=d.fwd?T.p[0]<T.spine+(T.leaf[1]-T.leaf[0])*0.5:T.p[0]>T.spine-(T.leaf[1]-T.leaf[0])*0.5;
      var flick=d.fwd?d.vx<-0.45:d.vx>0.45,complete=(past||flick)&&e.type!=='pointercancel';
      var stop=d.fwd===complete?T.end:T.c,result=complete?d.target:d.from;
      var dist=Math.abs(stop[0]-T.p[0])/(2*g.pw);
      if(complete)sound(0.25+dist*0.4,0.6);
      animate(T.p,stop,Math.max(140,DUR*0.8*dist),0,function(){show(result,complete);end(complete);});
      return;
    }
    if(!d.moved&&d.zone){go(d.zone,false);return;}
    if(Math.abs(dx)>=40&&Math.abs(dx)>Math.abs(dy)*1.5)go(dx<0?1:-1,false);
  }
  function onKey(e){
    if(e.target.closest('input,textarea,select,pre,.tbl-wrap,.eq-body'))return;
    var k=e.key,dir=0;
    if(k==='ArrowRight'||k==='PageDown'||(k===' '&&!e.shiftKey))dir=1;
    else if(k==='ArrowLeft'||k==='PageUp'||(k===' '&&e.shiftKey))dir=-1;
    else if(k==='Home'){e.preventDefault();show(0,true);afterTurn(true);return;}
    else if(k==='End'){e.preventDefault();show(pages-1,true);afterTurn(true);return;}
    if(!dir||e.target.closest('button')&&k===' ')return;
    e.preventDefault();go(dir,true);
  }

  // ---------- Sound: paper rustle synthesised with Web Audio ----------
  var ac=null,noise=null,live=null;
  function audio(){
    if(soundBtn.getAttribute('aria-pressed')!=='true')return null;
    if(!ac){var AC=window.AudioContext||window.webkitAudioContext;if(!AC)return null;ac=new AC();
      var len=ac.sampleRate*2,buf=ac.createBuffer(1,len,ac.sampleRate),d=buf.getChannelData(0),last=0;
      for(var i=0;i<len;i++){var w=Math.random()*2-1;last=0.6*last+0.4*w;d[i]=Math.random()<0.002?w:last*0.9+w*0.1;}
      noise=buf;}
    if(ac.state==='suspended')ac.resume();
    return ac;
  }
  function chain(c,freq,q,gain){
    var src=c.createBufferSource();src.buffer=noise;src.loop=true;src.playbackRate.value=0.8+Math.random()*0.4;
    var bp=c.createBiquadFilter();bp.type='bandpass';bp.frequency.value=freq;bp.Q.value=q;
    var gn=c.createGain();gn.gain.value=0;src.connect(bp);bp.connect(gn);gn.connect(c.destination);src.start(0,Math.random()*1.5);
    return {src:src,bp:bp,gain:gn,g:gain};
  }
  // One turn: rustle with crinkles, the air of the moving leaf, the soft landing.
  function sound(dur,strength){
    var c=audio();if(!c)return;var t=c.currentTime;
    var rustle=chain(c,2600,0.7,0.16*strength),air=chain(c,500,0.5,0.10*strength),land=chain(c,260,0.8,0.22*strength);
    var gr=rustle.gain.gain;gr.setValueAtTime(0,t);gr.linearRampToValueAtTime(rustle.g,t+0.04);
    for(var s=0.06;s<dur*0.8;s+=0.025+Math.random()*0.04)gr.setValueAtTime(rustle.g*(0.35+Math.random()*0.9),t+s);
    gr.linearRampToValueAtTime(0,t+dur);rustle.bp.frequency.setValueAtTime(1800,t);rustle.bp.frequency.linearRampToValueAtTime(3600,t+dur*0.6);
    var ga=air.gain.gain;ga.setValueAtTime(0,t);ga.linearRampToValueAtTime(air.g,t+dur*0.45);ga.linearRampToValueAtTime(0,t+dur);
    var gl=land.gain.gain;gl.setValueAtTime(0,t+dur*0.9);gl.linearRampToValueAtTime(land.g,t+dur*0.95);gl.exponentialRampToValueAtTime(0.0001,t+dur+0.12);
    [rustle,air,land].forEach(function(n){n.src.stop(t+dur+0.2);});
  }
  function soundStart(){var c=audio();if(!c)return;live=chain(c,2400,0.6,0.2);}
  function soundSpeed(v){if(!live)return;live.gain.gain.setTargetAtTime(Math.min(0.22,v*0.12)*(0.6+Math.random()*0.6),ac.currentTime,0.03);
    live.bp.frequency.setTargetAtTime(1800+Math.min(2400,v*1800),ac.currentTime,0.05);}
  function soundStop(){if(!live)return;var n=live;live=null;n.gain.gain.setTargetAtTime(0,ac.currentTime,0.04);n.src.stop(ac.currentTime+0.3);}

  // ---------- Entering and leaving ----------
  function enter(keep){
    if(active)return;if(!view)build();
    var anchor=keep||(P.anchorBlock&&P.anchorBlock()||[])[0];
    root.setAttribute('data-view','book');view.hidden=false;
    // Footnotes leave the margin column first: they change the flow.
    if(P.placeSidenotes)P.placeSidenotes();
    win.appendChild(article);active=true;
    paginate();show(anchor?colOf(anchor):0,false);
    // Prepare the replicas while the reader looks at the page, so the first turn starts at once.
    (window.requestIdleCallback||function(f){return setTimeout(f,200);})(function(){if(active&&!busy)ensureReplicas();});
    if(P.syncToggle)P.syncToggle();
    syncButtons();view.focus({preventScroll:true});
    if(!observer){observer=new MutationObserver(function(){stale=true;clearTimeout(observer.t);observer.t=setTimeout(function(){if(active&&!busy)repaginate();},300);});}
    observer.observe(article,{subtree:true,childList:true,characterData:true});
  }
  function leave(){
    if(!active)return;
    var keep=blockOnPage(page);if(observer)observer.disconnect();
    active=false;layout.appendChild(article);view.hidden=true;root.removeAttribute('data-view');
    win.firstElementChild&&win.removeChild(win.firstElementChild);
    article.style.removeProperty('--bk-x');clearTocPages();
    if(P.syncToggle)P.syncToggle();if(P.placeSidenotes)P.placeSidenotes();
    if(keep)keep.scrollIntoView({block:'start'});
    if(P.onScroll)P.onScroll();syncButtons();
  }
  function repaginate(){var keep=blockOnPage(page);paginate();show(keep?colOf(keep):0,false);}
  function syncButtons(){
    doc.querySelectorAll('[data-view-pick]').forEach(function(b){b.setAttribute('aria-pressed',(b.getAttribute('data-view-pick')==='book')===active?'true':'false');});
    doc.querySelectorAll('.view-btn').forEach(function(b){b.setAttribute('aria-pressed',active?'true':'false');});
  }
  function pick(book){if(P.store)P.store('p2h-view',book?'book':'scroll');if(book)enter();else leave();}
  doc.querySelectorAll('[data-view-pick]').forEach(function(b){b.addEventListener('click',function(){pick(b.getAttribute('data-view-pick')==='book');});});
  doc.querySelectorAll('.view-btn').forEach(function(b){b.addEventListener('click',function(){pick(!active);});});
  // Links inside the article and the contents turn to the page of their target.
  doc.addEventListener('click',function(e){
    if(!active)return;var a=e.target.closest('a[href^="#"]');if(!a)return;
    var t=doc.getElementById(decodeURIComponent(a.getAttribute('href').slice(1)));if(!t||!article.contains(t))return;
    e.preventDefault();var target=first(colOf(t));
    if(target!==page)turnTo(target,false,Math.abs(target-page)===per);
  },true);
  var resizeTimer=0;
  addEventListener('resize',function(){if(!active)return;clearTimeout(resizeTimer);resizeTimer=setTimeout(repaginate,150);});
  if(doc.fonts&&doc.fonts.ready)doc.fonts.ready.then(function(){if(active)repaginate();});
  addEventListener('beforeprint',function(){if(active){leave();root.setAttribute('data-print-book','1');}});
  addEventListener('afterprint',function(){if(root.getAttribute('data-print-book')){root.removeAttribute('data-print-book');enter();}});
  if(root.getAttribute('data-view')==='book'){root.removeAttribute('data-view');enter(article.firstElementChild);}
  syncButtons();
  window.p2hBook={enter:enter,leave:leave,go:go,col:function(n){return colOf(n);},state:function(){return {page:page,pages:pages,per:per,active:active,busy:busy};},
    drag:function(a,b){return {begin:begin,render:render,T:function(){return T;},stage:stage};}};
})();
