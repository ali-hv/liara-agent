#!/usr/bin/env python3
"""Unprivileged development fallback: extract distro headers to a user cache.
Does not install packages, run maintainer scripts, or modify system files.
"""
import concurrent.futures, os, pathlib, re, subprocess
root=pathlib.Path.home()/'.cache/liara-launch/native-deps'
archives=root/'archives';prefix=root/'sysroot';archives.mkdir(parents=True,exist_ok=True);prefix.mkdir(exist_ok=True)
packages=['libwebkit2gtk-4.1-dev','libxdo-dev','libayatana-appindicator3-dev','librsvg2-dev','libdbus-1-dev']
plan=subprocess.check_output(['apt-get','-s','--no-install-recommends','install',*packages],text=True)
# Retain all missing packages and development packages. Use installed runtime libraries where possible.
names=[]
for line in plan.splitlines():
 if line.startswith('Inst '):
  p=line.split()[1]
  if '[' not in line.split('(')[0] or p.endswith('-dev') or '-dev-' in p:names.append(p)
def download(p):
 r=subprocess.run(['apt-get','download',p],cwd=archives,text=True,stdout=subprocess.PIPE,stderr=subprocess.STDOUT)
 return p,r.returncode,r.stdout[-500:]
with concurrent.futures.ThreadPoolExecutor(max_workers=6) as pool:
 for p,code,log in pool.map(download,names):
  if code:print('FAILED',p,log,flush=True)
  else:print('Downloaded',p,flush=True)
for package in archives.glob('*.deb'):subprocess.run(['dpkg-deb','-x',str(package),str(prefix)],check=True)
for pc in prefix.rglob('*.pc'):
 text=pc.read_text();text=text.replace('/usr',str(prefix)+'/usr');pc.write_text(text)
for p in prefix.rglob('*'):
 if p.is_symlink() and not p.exists():
  target=os.readlink(p)
  candidates=[pathlib.Path('/usr/lib/x86_64-linux-gnu')/pathlib.Path(target).name,pathlib.Path('/lib/x86_64-linux-gnu')/pathlib.Path(target).name]
  for c in candidates:
   if c.exists():p.unlink();p.symlink_to(c);break
lib=prefix/'usr/lib/x86_64-linux-gnu'
env=root/'env.sh'
env.write_text(f'export PKG_CONFIG_PATH="{lib}/pkgconfig:{prefix}/usr/share/pkgconfig${{PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}}"\nexport LIBRARY_PATH="{lib}${{LIBRARY_PATH:+:$LIBRARY_PATH}}"\nexport LD_LIBRARY_PATH="{lib}${{LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}}"\n')
print('Environment:',env,flush=True)
