(function(){
  var root=document.documentElement;
  try{var t=localStorage.getItem('p2h-theme');if(t)root.setAttribute('data-theme',t);}catch(e){}
  var btn=document.getElementById('theme');
  if(btn)btn.addEventListener('click',function(){
    var dark=root.getAttribute('data-theme')==='dark'||(!root.getAttribute('data-theme')&&matchMedia('(prefers-color-scheme:dark)').matches);
    var next=dark?'light':'dark';root.setAttribute('data-theme',next);
    try{localStorage.setItem('p2h-theme',next);}catch(e){}
  });
  var bar=document.getElementById('progress');
  function prog(){var h=document.documentElement;var max=h.scrollHeight-h.clientHeight;bar.style.width=(max>0?h.scrollTop/max*100:0)+'%';}
  addEventListener('scroll',prog,{passive:true});prog();
  var toc=document.querySelector('nav.toc');
  if(toc){
    var title=toc.querySelector('.toc-title');
    if(title)title.addEventListener('click',function(){toc.classList.toggle('open');});
    var links={};toc.querySelectorAll('a').forEach(function(a){links[a.getAttribute('href').slice(1)]=a;});
    var heads=[].slice.call(document.querySelectorAll('article h2[id],article h3[id],article h4[id]')).filter(function(h){return links[h.id];});
    var cur=null;
    function spy(){
      var y=scrollY+120,best=null;
      for(var i=0;i<heads.length;i++){if(heads[i].offsetTop<=y)best=heads[i];else break;}
      if(best!==cur){if(cur)links[cur.id].classList.remove('active');cur=best;
        if(cur){var a=links[cur.id];a.classList.add('active');
          if(toc.scrollHeight>toc.clientHeight){var r=a.offsetTop-toc.clientHeight/2;toc.scrollTop=r;}}}
    }
    addEventListener('scroll',spy,{passive:true});spy();
  }
  var lb=document.getElementById('lightbox');
  if(lb){
    var big=lb.querySelector('img');
    document.querySelectorAll('figure.float img').forEach(function(im){
      im.addEventListener('click',function(){big.src=im.src;big.alt=im.alt;lb.classList.add('open');});
    });
    lb.addEventListener('click',function(){lb.classList.remove('open');});
    addEventListener('keydown',function(e){if(e.key==='Escape')lb.classList.remove('open');});
  }
})();
