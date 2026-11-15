# Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
# SPDX-License-Identifier: GPL-3.0-or-later

# Writes crates/rk-render/src/xukmi_defaults.rs from the shader bundle of
# Vanilla Plus (abdata/chara/xukmi/shaders/vanillaplus.unity3d in its zipmod).
# Usage: python -I tools/gen_xukmi_defaults.py <bundle> <out.rs>
# Needs UnityPy.
import struct
import sys

import UnityPy

# ShaderLab property types and the flag [Gamma] sets.
COLOR, VECTOR, TEXTURE = 0, 1, 4
GAMMA = 32

env = UnityPy.load(sys.argv[1])
tables = {}
for o in env.objects:
    if o.type.name != 'Shader':
        continue
    pf = o.read_typetree()['m_ParsedForm']
    name = pf['m_Name']
    if not name.startswith('xukmi/') or 'Debug' in name:
        continue
    rows = []
    for p in pf['m_PropInfo']['m_Props']:
        ty = p['m_Type']
        if ty == TEXTURE:
            continue
        d = [p['m_DefValue[%d]' % k] for k in range(4)]
        if ty not in (COLOR, VECTOR):
            d = [d[0], 0.0, 0.0, 0.0]
        color = ty == COLOR or (ty == VECTOR and p['m_Flags'] & GAMMA != 0)
        if not color and not any(d):
            continue
        rows.append((p['m_Name'], color, d))
    tables[name] = rows


def f32(x):
    return struct.unpack('<f', struct.pack('<f', x))[0]


def lit(x):
    # The shortest decimal that reads back as the same 32-bit float.
    for digits in range(1, 10):
        s = '%.*g' % (digits, x)
        if f32(float(s)) == f32(x):
            break
    s = repr(float(s))
    return s if ('.' in s or 'e' in s) else s + '.0'


out = [
    '// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)',
    '// SPDX-License-Identifier: GPL-3.0-or-later',
    '',
    '//! Property defaults of the KKShadersPlus shaders, read from the property',
    '//! tables of `vanillaplus.unity3d` (Vanilla Plus 1.7.2). One row per property',
    '//! that is a colour or has a default other than zero: name, whether Unity',
    '//! hands it over linearised (a Color property, or a Vector marked [Gamma]),',
    '//! and the default as stored. Textures are left out.',
    '//! Written by `tools/gen_xukmi_defaults.py`.',
    '',
    "pub type Row = (&'static str, bool, [f32; 4]);",
]
seen = {}
names = {}
for name in sorted(tables):
    key = repr(tables[name])
    ident = name.split('/')[1].upper().replace('PLUS', '_PLUS').replace('TESS', '_TESS').replace('REFLECT', '_REFLECT')
    if key in seen:
        names[name] = seen[key]
        continue
    seen[key] = ident
    names[name] = ident
    out.append('')
    out.append('#[rustfmt::skip]')
    out.append('pub const %s: &[Row] = &[' % ident)
    for n, c, d in tables[name]:
        out.append('    ("%s", %s, [%s]),' % (n, 'true' if c else 'false', ', '.join(lit(x) for x in d)))
    out.append('];')
out.append('')
out.append('/// The table of a shader by its Unity name.')
out.append("pub fn table(shader: &str) -> Option<&'static [Row]> {")
out.append('    Some(match shader {')
by = {}
for name in sorted(names):
    by.setdefault(names[name], []).append(name)
for ident in sorted(by):
    out.append('        %s => %s,' % (' | '.join('"%s"' % n for n in by[ident]), ident))
out.append('        _ => return None,')
out.append('    })')
out.append('}')
open(sys.argv[2], 'w', encoding='utf-8').write('\n'.join(out) + '\n')
print(len(out), 'lines')
