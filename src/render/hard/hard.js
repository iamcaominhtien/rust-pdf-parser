(function(){
  // Stretch every text-layer line to the exact width it has in the PDF so
  // that selection highlights line up with the rendered page.
  function fit(page){
    if(page.dataset.fit)return;page.dataset.fit='1';
    var w=page.getBoundingClientRect().width,ls=page.querySelectorAll('.l'),m=[],i;
    for(i=0;i<ls.length;i++)m.push(ls[i].getBoundingClientRect().width);
    for(i=0;i<ls.length;i++){var t=Number.parseFloat(ls[i].dataset.w)/100*w;if(m[i]>0&&t>0)ls[i].style.transform='scaleX('+(t/m[i]).toFixed(4)+')';}
  }
  var pages=document.querySelectorAll('.page');
  if('IntersectionObserver' in window){
    var io=new IntersectionObserver(function(es){es.forEach(function(e){if(e.isIntersecting){fit(e.target);io.unobserve(e.target);}});},{rootMargin:'800px'});
    pages.forEach(function(p){io.observe(p);});
  }else{pages.forEach(fit);}
  var root=document.documentElement,zoom=1.5;
  function set(z){zoom=Math.max(.5,Math.min(4,z));root.style.setProperty('--zoom',zoom);document.body.classList.remove('fit');}
  var b=function(id,f){var e=document.getElementById(id);if(e)e.addEventListener('click',f);};
  b('zin',function(){set(zoom*1.2);});
  b('zout',function(){set(zoom/1.2);});
  b('zfit',function(){document.body.classList.add('fit');});
})();
