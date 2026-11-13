// Book view: the article laid out in pages (CSS columns over the one article
// DOM) and turned like paper. A turn bends the page along a fold line (the
// perpendicular bisector between the corner's rest position and where it is
// now), shows the next page on the back of the folded part, shades the fold
// and casts a shadow on the page beneath; the corner can be dragged by hand.
// A recorded page turn (CC0, sounds/README.md) follows the motion.
// The layout is helped by script where CSS columns cannot decide: figures
// and tables are scaled to a page (and sent on past text that fills a gap),
// footnotes stand after their mark, headings keep to what follows.
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


  // ---------- Footnotes: on the page of their mark ----------
  // A note stands after the block that holds its mark (it is set where the
  // paper had it, often far from the mark). The scroll view gets them back.
  var moved=[];
  function placeNotes(){
    [].forEach.call(article.querySelectorAll('aside.footnote'),function(note){
      var mark=P.markFor&&P.markFor(note);if(!mark||note.contains(mark))return;
      var blk=mark;while(blk&&blk.parentNode!==article)blk=blk.parentNode;
      if(!blk||blk===note)return;
      var at=blk;while(at.nextElementSibling&&at.nextElementSibling.classList.contains('footnote')&&at.nextElementSibling!==note)at=at.nextElementSibling;
      if(at.nextElementSibling===note||at===note)return;
      var hole=doc.createComment('');note.parentNode.replaceChild(hole,note);
      at.parentNode.insertBefore(note,at.nextSibling);moved.push([note,hole]);
    });
  }
  function restoreNotes(){
    while(moved.length){var m=moved.pop();m[1].parentNode.replaceChild(m[0],m[1]);}
  }


  // ---------- Headings: never the last thing on a page ----------
  // The browser keeps a heading with what follows it, but gives up when
  // two headings follow each other. A heading (or a run of them) whose
  // next block starts on a later page starts a page itself.
  function keepHeads(art){
    [].forEach.call(art.querySelectorAll('[data-bkb]'),function(h){h.removeAttribute('data-bkb');h.style.breakBefore='';});
    for(var round=0;round<4;round++){
      var changed=false;
      [].forEach.call(art.querySelectorAll('h2,h3,h4,h5'),function(h){
        var next=h.nextElementSibling,hr=h.getClientRects(),nr=next&&next.getClientRects();
        if(!hr.length||!nr||!nr.length||/^H[2-5]$/.test(next.tagName))return;
        if(colOf(next)<=Math.floor((hr[hr.length-1].left-art.getBoundingClientRect().left+2)/step))return;
        var start=h;while(start.previousElementSibling&&/^H[2-5]$/.test(start.previousElementSibling.tagName))start=start.previousElementSibling;
        if(start.hasAttribute('data-bkb')||start.getBoundingClientRect().top-art.getBoundingClientRect().top<4)return;
        start.setAttribute('data-bkb','');start.style.breakBefore='column';changed=true;
      });
      if(!changed)break;
    }
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
    // A page cannot scroll sideways: wide tables shrink further than in the scroll view.
    if(P.fitTables)P.fitTables(art,0.55);
    fitFloats(art);fillGaps(art);keepHeads(art);if(P.cues)P.cues(art);if(observer)observer.takeRecords();
    pages=Math.max(1,Math.round((art.scrollWidth+(step-g.tw))/step));
    // Entries of a bibliography that were mistaken for headings do not head a page.
    heads=[].filter.call(art.querySelectorAll('h2,h3,h4'),function(h){return !/^\[\d+\]/.test(h.textContent.trim());}).map(function(h){
      var n=h.querySelector('.secno'),text=n?n.textContent+' '+h.textContent.slice(n.textContent.length):h.textContent;
      return {col:colOf(h),text:text.replace(/\s+/g,' ').trim(),el:h};});
    tocPages();
    stale=true;
  }

  // ---------- Floats: a figure or table stays whole, with its caption ----------
  // A figure (a picture, a table, an algorithm) taller than the page is
  // scaled down until it and its caption fit one page, and one that would
  // leave most of a page empty by moving on is scaled to the room left
  // (down to GAP_MIN of its size). A float that cannot shrink enough
  // (a table of many rows) may split, the caption staying with its first rows.
  var GAP_MIN=0.55,probe=null;
  function probeBox(){
    if(!probe){probe=el('div','bk-probe');probe.setAttribute('aria-hidden','true');}
    if(probe.parentNode!==win)win.appendChild(probe);
    probe.style.width=g.tw+'px';return probe;
  }
  // The float scaled by k (a copy, laid out alone in a column of the text's width).
  function floatH(fig,base,k){
    var box=probeBox(),c=fig.cloneNode(true);
    box.textContent='';box.appendChild(c);
    if(k!=null)scale(c,base,k);
    var h=c.getBoundingClientRect().height;box.textContent='';return h;
  }
  function scale(root,base,k){
    [].forEach.call(root.querySelectorAll('img'),function(im,i){
      if(base.w[i]){im.setAttribute('data-bkw','');im.style.setProperty('--bk-w',Math.max(base.w[i]*k,base.lo[i]||0).toFixed(1)+'px');}});
    [].forEach.call(root.querySelectorAll('.tbl-wrap>table'),function(t,i){t.style.fontSize=(base.f[i]*k).toFixed(2)+'%';});
    [].forEach.call(root.querySelectorAll('.algo'),function(a){a.style.zoom=k<1?k.toFixed(3):'';});
  }
  // The sizes of the float before it is scaled, and the smallest scale that still reads.
  function baseOf(fig){
    var box=probeBox(),c=fig.cloneNode(true),base={w:[],lo:[],f:[],min:1};
    box.textContent='';box.appendChild(c);
    [].forEach.call(c.querySelectorAll('img'),function(im){
      var r=im.getBoundingClientRect(),cs=getComputedStyle(im.parentNode),room=im.parentNode.clientWidth-parseFloat(cs.paddingLeft)-parseFloat(cs.paddingRight);
      base.w.push(Math.min(r.width,room)||0);base.min=Math.min(base.min,.15);
      // A picture of a table is not shrunk below 70% of its size: it scrolls instead.
      var em=/em$/.test(im.style.width)&&im.parentNode.classList.contains('tbl-wrap')?parseFloat(im.style.width)*parseFloat(getComputedStyle(im).fontSize):0;
      base.lo.push(em?Math.min(.7*em,r.width):0);
      // It may be wider than the column: its box then has a scroll bar under it, which takes room.
      if(em&&.7*em>room+1)base.wide=true;});
    [].forEach.call(c.querySelectorAll('.tbl-wrap>table'),function(t){base.f.push(parseFloat(t.style.fontSize)||100);base.min=Math.min(base.min,.5);});
    if(c.querySelector('.algo'))base.min=Math.min(base.min,.6);
    base.scalable=base.w.length||base.f.length||!!c.querySelector('.algo');
    box.textContent='';return base;
  }
  // The largest scale in [lo, hi] at which the float is no taller than avail (0: none).
  function solve(fig,base,avail,lo,hi){
    if(floatH(fig,base,hi)<=avail)return hi;
    if(floatH(fig,base,lo)>avail)return 0;
    for(var i=0;i<7;i++){var m=(lo+hi)/2;if(floatH(fig,base,m)<=avail)lo=m;else hi=m;}
    return lo;
  }
  function clearFloats(art){
    [].forEach.call(art.querySelectorAll('img[data-bkw]'),function(im){im.removeAttribute('data-bkw');im.style.removeProperty('--bk-w');});
    [].forEach.call(art.querySelectorAll('.algo'),function(a){a.style.zoom='';});
    [].forEach.call(art.querySelectorAll('figure.float'),function(f){f.classList.remove('bk-split','bk-box');var w=f.querySelector('.tbl-wrap');if(w)w.style.maxHeight='';f._bk=null;f._k=1;f._moves=0;});
  }
  // The room a float has on a page of its own: the page less the headings
  // before it, which must not be left at the foot of the page before.
  function pageRoom(fig){
    var room=g.th-1,sum=0,p=fig.previousElementSibling;
    while(p&&/^H[2-5]$/.test(p.tagName)){sum+=p.getBoundingClientRect().height;p=p.previousElementSibling;}
    return sum?room-sum-parseFloat(getComputedStyle(fig).marginTop):room;
  }
  // A float too tall for a page splits after its first rows, unless it is
  // also wider than the column: a box that scrolls cannot be split, so such
  // a table stays whole in a box that scrolls both ways.
  function split(f){
    var wrap=f.querySelector('.tbl-wrap');
    f.classList.add('bk-split');
    var inner=wrap&&wrap.querySelector('table,img');
    if(!inner||inner.offsetWidth<=g.tw+1)return;
    f.classList.remove('bk-split');f.classList.add('bk-box');
    // Its caption (and the gap) laid out alone: what the box leaves of the page.
    var box=probeBox(),c=f.cloneNode(true),cw=c.querySelector('.tbl-wrap');
    cw.parentNode.removeChild(cw);box.textContent='';box.appendChild(c);
    var extra=c.getBoundingClientRect().height;box.textContent='';
    wrap.style.maxHeight=Math.max(120,pageRoom(f)-extra-g.fs-2*parseFloat(getComputedStyle(f).marginBottom)-20)+'px';
    f.style.display='none';void f.offsetHeight;f.style.display='';
  }
  function fitFloats(art){
    restoreFloats();clearFloats(art);
    [].forEach.call(art.querySelectorAll('figure.float'),function(f){
      var base=baseOf(f);f._bk=base;if(!base.scalable){if(floatH(f,base,null)>g.th)split(f);return;}
      var bar=base.wide?20:0,room=pageRoom(f)-bar,k=solve(f,base,room,base.min,1);
      if(!k&&room<g.th-1-bar){room=g.th-1-bar;k=solve(f,base,room,base.min,1);}
      if(!k){scale(f,base,base.min);f._k=base.min;split(f);return;}
      scale(f,base,k);f._k=k;
    });
  }
  // A float that starts a page after a page left a good part empty is
  // scaled to that room when it can be without shrinking too far; else it
  // moves on, behind the text that fills the room (as a float does in print).
  var floated=[];
  function restoreFloats(){while(floated.length){var m=floated.pop();m[0]._hole=null;m[1].parentNode.replaceChild(m[0],m[1]);}}
  function fragH(n){var h=0;[].forEach.call(n.getClientRects(),function(r){h+=r.height;});return h+parseFloat(getComputedStyle(n).marginBottom);}
  function sendOn(f,gap){
    var sum=0,n=f.nextElementSibling,at=null,count=0;
    while(n&&count<16&&!/^H[12]$/.test(n.tagName)&&!n.matches('footer,.colophon')){
      if(!n.classList.contains('footnote')){sum+=fragH(n);if(!/^H/.test(n.tagName))at=n;count++;if(sum>=gap)break;}
      n=n.nextElementSibling;
    }
    // Short of text before the end of the section: the float still moves behind what there is.
    if(!at||sum<gap*(!n||count<16&&sum<gap?0.3:0.9))return false;
    while(at.nextElementSibling&&at.nextElementSibling.classList.contains('footnote'))at=at.nextElementSibling;
    if(!f._hole){var hole=doc.createComment('');f.parentNode.insertBefore(hole,f);f._hole=hole;floated.push([f,hole]);}
    at.parentNode.insertBefore(f,at.nextSibling);return true;
  }
  function fillGaps(art){
    for(var round=0;round<8;round++){
      var top=art.getBoundingClientRect().top,todo=[];
      [].forEach.call(art.querySelectorAll('figure.float'),function(f){
        var rs=f.getClientRects(),prev=f.previousElementSibling;
        if(!rs.length||!prev||f.classList.contains('bk-split')||!f._bk||!f._bk.scalable)return;
        var c=colOf(f);if(c===0||rs[0].top-top>6)return;
        var pr=prev.getClientRects(),last=pr[pr.length-1];if(!last||colOf(prev)>=c)return;
        var gap=g.th-(last.bottom-top)-parseFloat(getComputedStyle(f).marginTop)-2;
        if(gap<g.th*0.2)return;
        todo.push([f,gap,g.th-(last.bottom-top)]);
      });
      var changed=false;
      todo.forEach(function(t){
        var f=t[0],k=f._k>GAP_MIN?solve(f,f._bk,t[1]-(f._bk.wide?20:0),GAP_MIN,f._k):0;
        if(k){if(k<f._k){scale(f,f._bk,k);f._k=k;changed=true;}}
        else if((f._moves=(f._moves||0)+1)<=6&&t[2]>=g.th*0.35&&sendOn(f,t[2]))changed=true;
      });
      if(!changed)break;
    }
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
      if(complete)soundFall(Math.max(140,DUR*0.8*dist)/1000,0.8);
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

  // ---------- Sound: a recorded page turn, shaped by the motion ----------
  // The leaf lifts during the first half of the recording and lands at
  // LAND seconds; the second half (from FALL) is the fall alone.
  var LAND=0.8,FALL=0.5,ac=null,buf=null,live=null;
  // Decoded once, when the book opens (decoding needs no user gesture).
  function loadSound(){
    if(buf||loadSound.busy||!window.p2hPageSound)return;
    var OAC=window.OfflineAudioContext||window.webkitOfflineAudioContext;if(!OAC)return;
    loadSound.busy=true;
    try{
      var bin=atob(window.p2hPageSound),bytes=new Uint8Array(bin.length);
      for(var i=0;i<bin.length;i++)bytes[i]=bin.charCodeAt(i);
      var p=new OAC(1,1,44100).decodeAudioData(bytes.buffer,function(b){buf=b;},function(){});
      if(p&&p.catch)p.catch(function(){});
    }catch(e){}
  }
  function audio(){
    if(soundBtn.getAttribute('aria-pressed')!=='true'||!buf)return null;
    if(!ac){var AC=window.AudioContext||window.webkitAudioContext;if(!AC)return null;ac=new AC();}
    if(ac.state==='suspended')ac.resume();
    return ac;
  }
  function clamp(v,a,b){return Math.max(a,Math.min(b,v));}
  function vary(){return 0.94+Math.random()*0.12;}
  // Plays the recording from `offset` at `rate` and `gain`; with `until`,
  // fades out and stops then (seconds from now).
  function play(offset,rate,gain,until){
    var c=audio();if(!c)return null;var t=c.currentTime;
    var src=c.createBufferSource(),g=c.createGain();
    src.buffer=buf;src.playbackRate.value=rate;g.gain.setValueAtTime(gain,t);
    if(until){g.gain.setValueAtTime(gain,t+until*0.8);g.gain.linearRampToValueAtTime(0,t+until);src.stop(t+until+0.05);}
    src.connect(g);g.connect(c.destination);src.start(t,offset);
    return {src:src,gain:g};
  }
  // A whole turn of `dur` seconds: the leaf lands when the turn ends.
  function sound(dur,strength){play(0,clamp(LAND/dur,0.9,1.25)*vary(),0.55*strength*vary());}
  // A page let go: only its fall, landing after `dur` seconds.
  function soundFall(dur,strength){play(FALL,clamp((LAND-FALL)/dur,0.85,1.25)*vary(),0.5*strength*vary());}
  // Dragging the corner: the paper sounds as fast as the hand moves.
  function soundStart(){live=play(0,vary(),0);}
  function soundSpeed(v){if(live)live.gain.gain.setTargetAtTime(Math.min(0.5,v*0.4),ac.currentTime,0.04);}
  function soundStop(){if(!live)return;var n=live;live=null;n.gain.gain.setTargetAtTime(0,ac.currentTime,0.04);n.src.stop(ac.currentTime+0.3);}

  // ---------- Entering and leaving ----------
  function enter(keep){
    if(active)return;if(!view)build();
    loadSound();
    var anchor=keep||(P.anchorBlock&&P.anchorBlock()||[])[0];
    root.setAttribute('data-view','book');view.hidden=false;
    // Footnotes leave the margin column first: they change the flow.
    if(P.placeSidenotes)P.placeSidenotes();
    win.appendChild(article);active=true;placeNotes();
    paginate();show(anchor?colOf(anchor):0,false);
    // Prepare the replicas while the reader looks at the page, so the first turn starts at once.
    (window.requestIdleCallback||function(f){return setTimeout(f,200);})(function(){if(active&&!busy)ensureReplicas();});
    if(P.syncToggle)P.syncToggle();
    syncButtons();view.focus({preventScroll:true});
    if(!observer){
      observer=new MutationObserver(function(){repaginateSoon(300);});
      // Images have no height until they load, and lazy ones load only near the
      // viewport: load them all now and lay the pages out again as they arrive.
      article.addEventListener('load',function(e){if(e.target.tagName==='IMG')repaginateSoon(150);},true);
    }
    [].forEach.call(article.querySelectorAll('img[loading="lazy"]'),function(img){img.loading='eager';});
    observer.observe(article,{subtree:true,childList:true,characterData:true});
  }
  function repaginateSoon(ms){
    stale=true;clearTimeout(repaginateSoon.t);
    repaginateSoon.t=setTimeout(function(){if(!active)return;if(busy)repaginateSoon(ms);else repaginate();},ms);
  }
  function leave(){
    if(!active)return;
    var keep=blockOnPage(page);if(observer)observer.disconnect();
    active=false;restoreFloats();restoreNotes();clearFloats(article);layout.appendChild(article);view.hidden=true;root.removeAttribute('data-view');
    win.firstElementChild&&win.removeChild(win.firstElementChild);
    article.style.removeProperty('--bk-x');clearTocPages();
    if(P.syncToggle)P.syncToggle();if(P.placeSidenotes)P.placeSidenotes();if(P.fitScroll)P.fitScroll();
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
  window.p2hBook={enter:enter,leave:leave,go:go,to:function(p){turnTo(first(p),false,false);},col:function(n){return colOf(n);},state:function(){return {page:page,pages:pages,per:per,active:active,busy:busy};},
    drag:function(a,b){return {begin:begin,render:render,T:function(){return T;},stage:stage};}};
})();
