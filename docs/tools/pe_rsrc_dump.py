#!/usr/bin/env python3
"""Reverse Modbus Poll's .rsrc: dialogs (with control trees), menus, string
tables, accelerators, version. Emits a human-readable UI map."""
import struct, sys

F = open('/tmp/mbp_extract/mbpoll.exe','rb').read()
e = struct.unpack_from("<I", F, 0x3C)[0]
assert F[e:e+4] == b"PE\0\0"
coff = e+4
nsec = struct.unpack_from("<H", F, coff+2)[0]
optsz = struct.unpack_from("<H", F, coff+16)[0]
opt = coff+20
magic = struct.unpack_from("<H", F, opt)[0]
dd = opt + (112 if magic == 0x20b else 96)
rsrc_rva, rsrc_sz = struct.unpack_from("<II", F, dd+8*2)
sec = opt+optsz
SECS = []
for i in range(nsec):
    b = sec+40*i
    nm = F[b:b+8].rstrip(b"\0").decode('latin1')
    vs, va, rs, pr = struct.unpack_from("<IIII", F, b+8)
    SECS.append((nm, va, vs, pr, rs))

def r2o(rva):
    for nm, va, vs, pr, rs in SECS:
        if va <= rva < va+max(vs, rs): return pr+(rva-va)

RS = r2o(rsrc_rva)

def u16(o): return struct.unpack_from("<H", F, o)[0]
def u32(o): return struct.unpack_from("<I", F, o)[0]

NAMES = {1:"CURSOR",2:"BITMAP",3:"ICON",4:"MENU",5:"DIALOG",6:"STRING",9:"ACCEL",
         12:"GRPCURSOR",14:"GRPICON",16:"VERSION",24:"MANIFEST",240:"TYP240",241:"TYP241"}

def walk(off, depth, tname, res):
    chars, ts, maj, minr, nnamed, nid = struct.unpack_from("<IIHHHH", F, RS+off)
    for i in range(nnamed+nid):
        eo = RS+off+16+8*i
        nidv, od = struct.unpack_from("<II", F, eo)
        if nidv & 0x80000000:
            so = RS+(nidv & 0x7fffffff); ln = u16(so)
            label = F[so+2:so+2+ln*2].decode('utf-16le', errors='replace')
        else:
            label = NAMES.get(nidv, str(nidv)) if depth == 0 else nidv
        if od & 0x80000000:
            walk(od & 0x7fffffff, depth+1, label if depth == 0 else tname, res)
        else:
            do = RS+(od & 0x7fffffff)
            drva, dsz, cp, _ = struct.unpack_from("<IIII", F, do)
            res.append((tname, label, r2o(drva), dsz))

RES = []
walk(0, 0, "", RES)

# ---- template readers -------------------------------------------------------
def sz_or_ord(o):
    """null-terminated UTF-16, or 0xFFFF+ordinal. returns (text, newoffset)"""
    w = u16(o)
    if w == 0x0000: return "", o+2
    if w == 0xFFFF: return f"#{u16(o+2)}", o+4
    st = o
    while u16(st) != 0: st += 2
    return F[o:st].decode('utf-16le', errors='replace'), st+2

CLASS = {0x80:"BUTTON",0x81:"EDIT",0x82:"STATIC",0x83:"LISTBOX",0x84:"SCROLLBAR",0x85:"COMBOBOX"}

def parse_dialog(o):
    dlgver = u16(o); sig = u16(o+2)
    ex = (dlgver == 1 and sig == 0xFFFF)
    if ex:
        helpid = u32(o+4); exstyle = u32(o+8); style = u32(o+12); cdit = u16(o+16)
        x,y,cx,cy = struct.unpack_from("<hhhh", F, o+18)
        p = o+26
        p = align(p, 4) if False else p
    else:
        style = u32(o); exstyle = u32(o+4); cdit = u16(o+8)
        x,y,cx,cy = struct.unpack_from("<hhhh", F, o+10)
        p = o+18
    menu, p = sz_or_ord(p)
    cls, p = sz_or_ord(p)
    title, p = sz_or_ord(p)
    if ex:
        ptsize = u16(p); weight = u16(p+2); italic = F[p+4]; charset = F[p+5]; p += 6
    items = []
    for _ in range(cdit):
        p = (p+3) & ~3
        if ex:
            ihelp = u32(p); iex = u32(p+4); istyle = u32(p+8)
            ix,iy,icx,icy = struct.unpack_from("<hhhh", F, p+12)
            iid = u32(p+20); p += 24
        else:
            iex = u32(p); istyle = u32(p+4)
            ix,iy,icx,icy = struct.unpack_from("<hhhh", F, p+8)
            iid = u16(p+16); p += 18
        icls, p = sz_or_ord(p)
        itext, p = sz_or_ord(p)
        extra = u16(p); p += 2 + extra
        if icls.startswith("#"):
            cname = CLASS.get(int(icls[1:]), icls)
        else:
            cname = icls or "?"
        items.append((iid, cname, itext, ix,iy,icx,icy, istyle))
    return dict(title=title, w=cx, h=cy, cdit=cdit, items=items)

def align(p, n): return (p+n-1) & ~(n-1)

def parse_menu(o):
    def items(p):
        out = []
        while True:
            opt = u16(p)
            if opt == 0: return out, p+2
            if opt & 0x10:
                text, no = sz_or_ord(p+2)
                sub, p2 = items(no)
                out.append(("POPUP", text, sub)); p = p2
            else:
                mid = u16(p+2); text, no = sz_or_ord(p+4)
                out.append(("ITEM", text, mid)); p = no
    return items(o+4)[0]

def parse_strings(o, sz):
    out = []
    p = o
    for i in range(16):
        if p+2 > o+sz: break
        ln = u16(p); p += 2
        s = F[p:p+ln*2].decode('utf-16le', errors='replace'); p += ln*2
        if s: out.append(s)
    return out

def parse_version(o):
    # find VS_FIXEDFILEINFO + StringFileInfo strings (rough scan for UTF-16 text)
    txt = F[o:o+4096].decode('utf-16le', errors='ignore')
    keep = ''.join(c if (32 <= ord(c) < 127 or c in "\n") else '\n' for c in txt)
    return [l for l in keep.split('\n') if len(l.strip()) > 1][:40]

mode = sys.argv[1] if len(sys.argv) > 1 else "all"

if mode in ("all", "dialogs"):
    print("=========== DIALOGS ===========")
    for t, name, o, sz in RES:
        if t != "DIALOG": continue
        try:
            d = parse_dialog(o)
        except Exception as ex:
            print(f"DLG {name}: parse error {ex}"); continue
        print(f"\nDLG {name}  \"{d['title']}\"  size={d['w']}x{d['h']}l units  items={d['cdit']}")
        for (iid, cls, txt, ix,iy,icx,icy, st) in d['items']:
            print(f"    [{iid:>5}] {cls:<9} ({ix},{iy},{icx}x{icy}) \"{txt}\"")
if mode in ("all", "menus"):
    print("\n=========== MENUS ===========")
    for t, name, o, sz in RES:
        if t != "MENU": continue
        print(f"\nMENU {name}:")
        def show(items, ind=4):
            for kind, s, extra in items:
                if kind == "POPUP":
                    print(" "*ind + f"[{s}]")
                    show(extra, ind+4)
                else:
                    print(" "*ind + f"{s!r}  cmd={extra}")
        try:
            show(parse_menu(o))
        except Exception as ex:
            print("   parse error", ex)
if mode in ("all", "strings"):
    print("\n=========== STRINGS ===========")
    for t, name, o, sz in sorted([r for r in RES if r[0]=="STRING"], key=lambda r: r[1]):
        try:
            ss = parse_strings(o, sz)
        except Exception as ex:
            ss = [f"<err {ex}>"]
        base_id = (int(name)-1)*16 if isinstance(name,int) else 0
        for i, s in enumerate(ss):
            print(f"  {base_id+i:>5}  {s}")
if mode in ("all", "version"):
    print("\n=========== VERSION ===========")
    for t, name, o, sz in RES:
        if t == "VERSION":
            for l in parse_version(o): print("  ", l)
