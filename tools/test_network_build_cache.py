"""Cache must invalidate on dependencies/flags, corrupted objects or explicit force."""
from pathlib import Path
import subprocess
import sys
import tempfile
from build_network import compile_cached

with tempfile.TemporaryDirectory(prefix='chaos-c-cache-') as directory:
    root=Path(directory);compiler=root/'compiler.py';obj=root/'test.o';count=root/'count'
    compiler.write_text('from pathlib import Path\nimport sys\np=Path(sys.argv[1]); n=Path(sys.argv[2]); n.write_text(str(int(n.read_text())+1) if n.exists() else "1"); p.write_bytes(b"object")\n')
    cmd=[sys.executable,str(compiler),str(obj),str(count)]
    assert compile_cached(cmd,obj,'headers-A')
    assert not compile_cached(cmd,obj,'headers-A')
    assert count.read_text()=='1'
    assert compile_cached(cmd,obj,'headers-B')
    assert compile_cached([*cmd,'new-flag'],obj,'headers-B')
    obj.write_bytes(b'corrupted')
    assert compile_cached([*cmd,'new-flag'],obj,'headers-B')
    assert compile_cached([*cmd,'new-flag'],obj,'headers-B',force=True)
    assert count.read_text()=='5'
    print('PASS network object cache: hits, header/flag invalidation, object corruption and forced rebuild')
