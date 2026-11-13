(function(){
  var doc=document,root=doc.documentElement,article=doc.getElementById('article');
  var live=doc.getElementById('live');
  function store(k,v){try{localStorage.setItem(k,v);}catch(e){}}
  function stored(k){try{return localStorage.getItem(k);}catch(e){return null;}}
  function wide(){return matchMedia('(min-width:1100px)').matches;}
  function book(){return root.getAttribute('data-view')==='book';}

  // Theme: Paper (default) / White / Dark. Nothing stored: the system decides
  // (Dark when it is dark) and changes follow it live.
  var osDark=matchMedia('(prefers-color-scheme:dark)');
  function effective(){var t=root.getAttribute('data-theme');return t||(osDark.matches?'dark':'paper');}
  function showTheme(){
    var t=effective();
    doc.querySelectorAll('[data-theme-pick]').forEach(function(b){b.setAttribute('aria-pressed',b.getAttribute('data-theme-pick')===t?'true':'false');});
    doc.querySelectorAll('input[name="p2h-theme"]').forEach(function(r){r.checked=r.value===t;});
  }
  function pick(t){root.setAttribute('data-theme',t);store('p2h-theme',t);showTheme();}
  doc.querySelectorAll('[data-theme-pick]').forEach(function(b){b.addEventListener('click',function(){pick(b.getAttribute('data-theme-pick'));});});
  doc.querySelectorAll('input[name="p2h-theme"]').forEach(function(r){r.addEventListener('change',function(){pick(r.value);});});
  if(osDark.addEventListener)osDark.addEventListener('change',showTheme);
  showTheme();
  var themeBtn=doc.querySelector('.theme-btn'),themeMenu=doc.getElementById('theme-menu');
  function closeThemeMenu(){if(!themeMenu.hidden){themeMenu.hidden=true;themeBtn.setAttribute('aria-expanded','false');}}
  if(themeBtn&&themeMenu){
    themeBtn.addEventListener('click',function(){var open=themeMenu.hidden;themeMenu.hidden=!open;themeBtn.setAttribute('aria-expanded',open?'true':'false');
      if(open){var c=themeMenu.querySelector('input:checked')||themeMenu.querySelector('input');if(c)c.focus();}});
    themeMenu.addEventListener('keydown',function(e){if(e.key==='Escape'){closeThemeMenu();themeBtn.focus();}});
    doc.addEventListener('click',function(e){if(!themeMenu.contains(e.target)&&!themeBtn.contains(e.target))closeThemeMenu();});
  }

  // Contents: a column on wide screens (shown or hidden, remembered), a
  // drawer below 1100px (closed on every load).
  var toc=doc.getElementById('toc'),toggle=doc.getElementById('toc-toggle'),drawer=doc.getElementById('toc-drawer');
  var layout=doc.querySelector('.layout');
  function syncToggle(){if(toggle)toggle.setAttribute('aria-expanded',wide()&&!book()&&root.getAttribute('data-toc')!=='hidden'?'true':'false');}
  // The first block that starts below the running head (else the one
  // crossing it) and its position, to keep it in place when lines rewrap.
  function anchorBlock(){
    var kids=article.children,head=48,crossing=null;
    for(var i=0;i<kids.length;i++){
      var r=kids[i].getBoundingClientRect();
      if(r.top>=head&&r.height>0)return [kids[i],r.top];
      if(!crossing&&r.bottom>head)crossing=[kids[i],r.top];
    }
    return crossing;
  }
  function openDrawer(){
    if(!drawer||!toc||!drawer.showModal)return;
    drawer.appendChild(toc);drawer.showModal();toggle.setAttribute('aria-expanded','true');
    var a=toc.querySelector('a.active')||toc.querySelector('a');if(a)a.focus();
  }
  if(drawer){
    drawer.addEventListener('close',function(){layout.insertBefore(toc,layout.firstChild);syncToggle();if(toggle)toggle.focus();});
    drawer.addEventListener('click',function(e){
      if(e.target===drawer||e.target.closest('.drawer-close')||e.target.closest('a'))drawer.close();
    });
  }
  if(!toc&&toggle)toggle.hidden=true;
  if(toggle&&toc)toggle.addEventListener('click',function(){
    if(!wide()||book()){openDrawer();return;}
    var keep=anchorBlock(),hidden=root.getAttribute('data-toc')==='hidden';
    if(hidden)root.removeAttribute('data-toc');else root.setAttribute('data-toc','hidden');
    store('p2h-toc',hidden?'shown':'hidden');syncToggle();
    if(keep)scrollBy(0,keep[0].getBoundingClientRect().top-keep[1]);
    placeSidenotes();
  });
  addEventListener('resize',function(){syncToggle();if(drawer&&drawer.open&&wide())drawer.close();});
  syncToggle();
  var filter=toc&&toc.querySelector('.toc-filter');
  if(filter){
    var fold=function(s){return s.normalize('NFD').replace(/[̀-ͯ]/g,'').toLowerCase();};
    filter.addEventListener('input',function(){var q=fold(filter.value.trim());
      toc.querySelectorAll('li').forEach(function(li){li.hidden=q!==''&&fold(li.textContent).indexOf(q)<0;});});
  }

  // Reading progress, active contents entry and the running head.
  var bar=doc.getElementById('progress'),rhSec=doc.querySelector('.rh-sec'),h1=doc.querySelector('h1.title');
  var links={};if(toc)toc.querySelectorAll('a').forEach(function(a){links[a.getAttribute('href').slice(1)]=a;});
  var heads=[].slice.call(article.querySelectorAll('h2[id],h3[id],h4[id]')).filter(function(h){return links[h.id];});
  var cur=null,ticking=false;
  function onScroll(){
    if(book())return;
    var max=root.scrollHeight-root.clientHeight;bar.style.width=(max>0?root.scrollTop/max*100:0)+'%';
    var y=110,best=null;
    for(var i=0;i<heads.length;i++){if(heads[i].getBoundingClientRect().top<=y)best=heads[i];else break;}
    if(best!==cur){
      if(cur&&links[cur.id])links[cur.id].classList.remove('active');
      cur=best;
      if(cur){var a=links[cur.id];a.classList.add('active');if(toc.scrollHeight>toc.clientHeight)toc.scrollTop=a.offsetTop-toc.clientHeight/2;}
    }
    if(rhSec){
      var showSec=cur&&!(h1&&h1.getBoundingClientRect().bottom>48);
      if(!showSec)rhSec.textContent='';
      else{var n=cur.querySelector('.secno');rhSec.innerHTML='';
        if(n){var b=doc.createElement('b');b.textContent=n.textContent;rhSec.appendChild(b);}
        rhSec.appendChild(doc.createTextNode(n?cur.textContent.slice(n.textContent.length):cur.textContent));}
    }
    ticking=false;
  }
  addEventListener('scroll',function(){if(!ticking){ticking=true;requestAnimationFrame(onScroll);}},{passive:true});
  onScroll();

  // Back to where a cross-reference was followed.
  var chip=doc.querySelector('.return'),from=null,chipTimer=0;
  article.addEventListener('click',function(e){
    var a=e.target.closest('a.xref');if(!a||!chip)return;
    from=a;chip.hidden=false;clearTimeout(chipTimer);chipTimer=setTimeout(function(){chip.hidden=true;},12000);
  });
  if(chip)chip.addEventListener('click',function(){
    chip.hidden=true;if(!from)return;from.scrollIntoView({block:'center'});from.focus();from=null;
  });

  // Footnotes in the margin column on very wide screens, next to their mark.
  function blockOf(el){while(el&&el.parentElement!==article)el=el.parentElement;return el;}
  function hasMark(el,text){var sups=el.querySelectorAll('sup');for(var j=0;j<sups.length;j++)if(sups[j].textContent.trim()===text)return true;return false;}
  function markFor(note){
    if(note.id){var link=article.querySelector('a[href="#'+CSS.escape(note.id)+'"]');if(link)return blockOf(link);}
    var m=note.querySelector('.fn-mark'),text=m&&m.textContent.trim();
    if(text){
      var back=note.previousElementSibling,fwd=note.nextElementSibling;
      for(var i=0;i<20&&(back||fwd);i++){
        if(back){if(!back.classList.contains('footnote')&&hasMark(back,text))return back;back=back.previousElementSibling;}
        if(fwd){if(!fwd.classList.contains('footnote')&&hasMark(fwd,text))return fwd;fwd=fwd.nextElementSibling;}
      }
    }
    var p=note.previousElementSibling;while(p&&p.classList.contains('footnote'))p=p.previousElementSibling;return p;
  }
  function placeSidenotes(){
    var notes=article.querySelectorAll('aside.footnote'),side=matchMedia('(min-width:1440px)').matches&&!book(),bottom=0;
    notes.forEach(function(n){n.classList.remove('side');n.style.top='';});
    if(!side)return;
    var placed=[];
    notes.forEach(function(n){var target=markFor(n);if(target)placed.push([target.offsetTop,n]);});
    placed.sort(function(a,b){return a[0]-b[0];});
    placed.forEach(function(item){
      var n=item[1];n.classList.add('side');var top=Math.max(item[0],bottom);n.style.top=top+'px';bottom=top+n.offsetHeight+12;
    });
  }
  addEventListener('resize',placeSidenotes);
  addEventListener('load',placeSidenotes);
  if(doc.fonts&&doc.fonts.ready)doc.fonts.ready.then(placeSidenotes);
  placeSidenotes();

  // Equations wider than the column scroll; show where they are clipped.
  function clipped(){doc.querySelectorAll('.eq-body').forEach(function(b){b.classList.toggle('is-clipped',b.scrollWidth>b.clientWidth+1);});}
  addEventListener('resize',clipped);addEventListener('load',clipped);clipped();

  // Code: copy the lines (numbers are CSS, never copied) and switch colour.
  function say(text){if(live){live.textContent=text;setTimeout(function(){if(live.textContent===text)live.textContent='';},2000);}}
  doc.querySelectorAll('figure.code').forEach(function(fig){
    var pre=fig.querySelector('pre.listing'),copy=fig.querySelector('.code-copy');
    if(copy)copy.addEventListener('click',function(){
      var text=[].map.call(pre.querySelectorAll('.l'),function(l){return l.textContent;}).join('\n');
      var done=function(){copy.setAttribute('data-state','copied');say('Copied');setTimeout(function(){copy.removeAttribute('data-state');},2000);};
      if(navigator.clipboard&&navigator.clipboard.writeText)navigator.clipboard.writeText(text).then(done,fallback);else fallback();
      function fallback(){var r=doc.createRange();r.selectNodeContents(pre);var s=getSelection();s.removeAllRanges();s.addRange(r);
        try{doc.execCommand('copy');done();}catch(e){}s.removeAllRanges();}
    });
  });
  function codeColour(){
    var pref=root.getAttribute('data-code-hl');
    doc.querySelectorAll('figure.code').forEach(function(fig){
      var pre=fig.querySelector('pre.listing'),b=fig.querySelector('.code-hl');
      var on=pref?pref==='on':pre.classList.contains('hl');if(b)b.setAttribute('aria-pressed',on?'true':'false');
    });
  }
  var hlPref=stored('p2h-code-hl');if(hlPref==='on'||hlPref==='off')root.setAttribute('data-code-hl',hlPref);
  doc.querySelectorAll('.code-hl').forEach(function(b){b.addEventListener('click',function(){
    var next=b.getAttribute('aria-pressed')==='true'?'off':'on';root.setAttribute('data-code-hl',next);store('p2h-code-hl',next);codeColour();
  });});
  codeColour();
  function boxes(){doc.querySelectorAll('.code-box').forEach(function(box){var pre=box.firstElementChild;
    box.classList.toggle('is-clipped',pre.scrollWidth>pre.clientWidth+1&&pre.scrollLeft+pre.clientWidth<pre.scrollWidth-1);});}
  doc.querySelectorAll('pre.listing').forEach(function(pre){pre.addEventListener('scroll',boxes,{passive:true});});
  addEventListener('resize',boxes);boxes();

  // All authors.
  var more=doc.querySelector('.au-more');
  if(more)more.addEventListener('click',function(){doc.querySelectorAll('.author[hidden]').forEach(function(a){a.hidden=false;});more.remove();});

  // Figures open full size.
  var lb=doc.getElementById('lightbox');
  if(lb){
    var big=lb.querySelector('img'),opener=null;
    doc.querySelectorAll('.plate img').forEach(function(im){
      im.tabIndex=0;im.setAttribute('role','button');
      var open=function(){opener=im;big.src=im.src;big.alt=im.alt;lb.classList.add('open');lb.tabIndex=-1;lb.focus();};
      im.addEventListener('click',open);
      im.addEventListener('keydown',function(e){if(e.key==='Enter'||e.key===' '){e.preventDefault();open();}});
    });
    var close=function(){if(!lb.classList.contains('open'))return;lb.classList.remove('open');if(opener)opener.focus();};
    lb.addEventListener('click',close);
    lb.addEventListener('keydown',function(e){if(e.key==='Escape')close();});
  }
  if(live)live.textContent='';
  // Shared with the book view (book.js).
  window.p2h={store:store,stored:stored,say:say,anchorBlock:anchorBlock,placeSidenotes:placeSidenotes,syncToggle:syncToggle,onScroll:onScroll};
})();
