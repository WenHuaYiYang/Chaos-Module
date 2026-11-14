#!/usr/bin/env python3
"""Production local file controller against the same POSIX adapter used by protocol tests."""
from pathlib import Path
import os,subprocess,tempfile
ROOT=Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix='chaos-local-files-') as work:
    w=Path(work);obj=[]
    for name in ['supervisor/tests/chaos_connect_host.c','supervisor/bridge/chaos_connect.c','supervisor/bridge/chaos_files.c']:
        out=w/(Path(name).stem+'.o');obj.append(str(out))
        subprocess.run(['clang','-std=c11','-D_DEFAULT_SOURCE','-fPIC','-O2','-I'+str(ROOT/'supervisor/bridge'),'-c',str(ROOT/name),'-o',str(out)],check=True)
    binary=w/'controller'
    subprocess.run(['rustc','--edition=2021',str(ROOT/'supervisor/tests/local_files_harness.rs'),*[arg for o in obj for arg in ['-C','link-arg='+o]],'-o',str(binary)],check=True)
    subprocess.run([str(binary)],env=dict(os.environ,CHAOS_CONNECT_TEST_ROOT=str(w/'fs')),check=True)
    binary=w/'path-tests'
    subprocess.run(['rustc','--test',str(ROOT/'supervisor/src/file_path.rs'),'-o',str(binary)],check=True)
    subprocess.run([str(binary)],check=True)
