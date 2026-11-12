(function(){
  var doc=document, cv=doc.createElement('canvas').getContext('2d');
  function clamp(v,a,b){return Math.max(a,Math.min(b,v));}
  function deepest(el){var e=el;while(e.firstElementChild)e=e.firstElementChild;return e;}

  // Give a line of text exactly the width it has in the PDF by adjusting
  // the space between words (and, if needed, between letters).
  // Rotated text is measured along its own baseline.
  function measure(el){el.style.wordSpacing='';el.style.letterSpacing='';return el.classList.contains('rt')?el.offsetWidth:el.getBoundingClientRect().width;}
  function apply(el,natural,pageW,fs){
    var target=parseFloat(el.dataset.w)/100*pageW;
    if(!(target>0)||!(natural>0)||!(fs>0))return;
    var d=target-natural,txt=el.textContent,gaps=(txt.match(/ /g)||[]).length,ws=0;
    if(gaps>0){ws=clamp(d/gaps,-0.14*fs,0.7*fs);el.style.wordSpacing=(ws/fs).toFixed(4)+'em';d-=ws*gaps;}
    var n=Array.from(txt).length;
    if(Math.abs(d)>0.3&&n>1){var ls=clamp(d/(n-1),-0.12*fs,0.25*fs);el.style.letterSpacing=(ls/fs).toFixed(4)+'em';}
  }
  function fitAll(els,pageW){
    var data=[],i;
    for(i=0;i<els.length;i++)data.push([els[i],measure(els[i]),parseFloat(getComputedStyle(els[i]).fontSize)]);
    for(i=0;i<data.length;i++)apply(data[i][0],data[i][1],pageW,data[i][2]);
  }

  // Glyphs that do not sit on the baseline in the usual way (big operators,
  // tall delimiters, radicals) are stretched onto the box their ink has in
  // the PDF.
  function fitInk(el,pr){
    var ink=el.dataset.ink.split(',').map(Number),cs=getComputedStyle(deepest(el)),fs=parseFloat(cs.fontSize);
    cv.font=cs.fontStyle+' '+cs.fontWeight+' '+fs+'px '+cs.fontFamily;
    var m=cv.measureText(el.textContent),iw=m.actualBoundingBoxLeft+m.actualBoundingBoxRight,ih=m.actualBoundingBoxAscent+m.actualBoundingBoxDescent;
    if(!(iw>0.01&&ih>0.01))return;
    var asc=m.fontBoundingBoxAscent!=null?m.fontBoundingBoxAscent:fs*0.89,desc=m.fontBoundingBoxDescent!=null?m.fontBoundingBoxDescent:fs*0.22;
    el.style.transform='';
    var er=el.getBoundingClientRect(),base=(er.height-(asc+desc))/2+asc;
    var tx=pr.left+ink[0]/100*pr.width,ty=pr.top+ink[1]/100*pr.height,tw=(ink[2]-ink[0])/100*pr.width,th=(ink[3]-ink[1])/100*pr.height;
    var sx=clamp(tw/iw,0.15,10),sy=clamp(th/ih,0.15,10);
    var dx=(tx-er.left)+m.actualBoundingBoxLeft*sx,dy=(ty-er.top)-(base-m.actualBoundingBoxAscent)*sy;
    el.style.transform='translate('+(dx/fs).toFixed(4)+'em,'+(dy/fs).toFixed(4)+'em) scale('+sx.toFixed(4)+','+sy.toFixed(4)+')';
  }

  function clonesOf(fb){return doc.querySelectorAll('.fb.cont[data-of="'+fb.dataset.block+'"]');}
  var original=new WeakMap(),done=new WeakSet(),busy=false;

  function fitBlock(fb){
    if(done.has(fb))return;done.add(fb);
    var page=fb.closest('.page'),w=page.getBoundingClientRect().width,lines=fb.querySelectorAll('.ln');
    fitAll(lines,w);
    clonesOf(fb).forEach(function(c){
      var cl=c.querySelectorAll('.ln');
      for(var i=0;i<cl.length&&i<lines.length;i++){cl[i].style.wordSpacing=lines[i].style.wordSpacing;cl[i].style.letterSpacing=lines[i].style.letterSpacing;}
    });
    original.set(fb,fb.textContent);
  }
  function fitPage(page){
    if(page.dataset.fit)return;page.dataset.fit='1';
    var pr=page.getBoundingClientRect();
    page.querySelectorAll('.fb').forEach(function(fb){
      if(fb.classList.contains('cont')){var src=doc.querySelector('.fb[data-block="'+fb.dataset.of+'"]');if(src)fitBlock(src);}
      else fitBlock(fb);
    });
    fitAll(page.querySelectorAll('.pl, .rt, .g:not([data-ink])'),pr.width);
    page.querySelectorAll('.g[data-ink]').forEach(function(g){fitInk(g,pr);});
  }

  // After its text changed, a block gives up the line breaks of the PDF and
  // flows inside its boxes, shrinking the type if the new text is longer.
  function reflow(fb){
    busy=true;
    var inner=fb.firstElementChild,boxes=[fb].concat([].slice.call(clonesOf(fb))),total=0;
    boxes.forEach(function(b){b.classList.add('rf');total+=b.getBoundingClientRect().height;});
    var lh=parseFloat(getComputedStyle(fb).lineHeight)||0,scale=1;
    inner.style.fontSize='';
    while(scale>0.62&&inner.scrollHeight>total+lh*0.3){scale-=0.03;inner.style.fontSize=Math.round(scale*100)+'%';}
    boxes.slice(1).forEach(function(b){var ci=b.firstElementChild;ci.innerHTML=inner.innerHTML;ci.style.fontSize=inner.style.fontSize;});
    setTimeout(function(){busy=false;},0);
  }
  var mo=new MutationObserver(function(recs){
    if(busy)return;
    var changed=new Set();
    recs.forEach(function(r){
      var n=r.target.nodeType===1?r.target:r.target.parentElement;if(!n||!n.closest)return;
      var fb=n.closest('.fb');if(fb&&!fb.classList.contains('cont'))changed.add(fb);
      var tb=n.closest('.tb');if(tb)tb.classList.add('rf');
      var pl=n.closest('.pl');if(pl){pl.style.wordSpacing='';pl.style.letterSpacing='';}
    });
    changed.forEach(function(fb){if(original.has(fb)&&fb.textContent!==original.get(fb))reflow(fb);});
  });

  var pages=doc.querySelectorAll('.page');
  if('IntersectionObserver' in window){
    var io=new IntersectionObserver(function(es){es.forEach(function(e){if(e.isIntersecting){fitPage(e.target);io.unobserve(e.target);}});},{rootMargin:'1200px'});
    pages.forEach(function(p){io.observe(p);});
  }else{pages.forEach(fitPage);}
  var main=doc.querySelector('.pages');
  if(main)mo.observe(main,{subtree:true,childList:true,characterData:true});
  window.paper2htmlFitAll=function(){pages.forEach(fitPage);};

  var root=doc.documentElement,zoom=1.5;
  function set(z){zoom=clamp(z,0.5,4);root.style.setProperty('--zoom',zoom);doc.body.classList.remove('fit');}
  function on(id,f){var e=doc.getElementById(id);if(e)e.addEventListener('click',f);}
  on('zin',function(){set(zoom*1.2);});
  on('zout',function(){set(zoom/1.2);});
  on('zfit',function(){doc.body.classList.add('fit');});
})();
