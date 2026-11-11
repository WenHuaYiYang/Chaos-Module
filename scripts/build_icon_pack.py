#!/usr/bin/env python3
"""Compatibility entry point; implementation lives in tools/build_icon_pack.py."""
from pathlib import Path
import runpy
import sys
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'tools'))
runpy.run_module('build_icon_pack',run_name='__main__')
