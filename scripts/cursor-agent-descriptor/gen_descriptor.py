"""Generate a FileDescriptorSet text proto from the Cursor Agent CLI bundle.

Reads the webpack bundle's compact protobuf-es specs (`static $(){return["Name|no name type..",refs]}`)
and enum helpers (`(0,c.QT)(runtime,pkg,"Name",[[no,"N"]],mode)`), resolves references across
modules, and emits the transitive closure of the given root messages. Protobuf facts only.

usage: python3 -I gen_descriptor.py <bundle_dir> <old_descriptor_text> > out.txt
"""
import collections
import glob
import json
import os
import re
import sys

bundle, old_text = sys.argv[1], sys.argv[2]
ROOTS = [
    "agent.v1.ConversationStateStructure",
    "agent.v1.ConversationTurnStructure",
    "agent.v1.UserMessage",
    "agent.v1.ConversationStep",
    "agent.v1.ShellCommand",
    "agent.v1.ShellOutput",
]
ID = r"[\w$]+"
modules = {}  # path -> source
for f in sorted(glob.glob(os.path.join(bundle, "*.js"))):
    src = open(f, encoding="utf-8", errors="replace").read()
    heads = list(re.finditer(r'[{,]"([^"]+)"\((?:t,e,r|t,e|t)\)\{', src))
    for i, h in enumerate(heads):
        end = heads[i + 1].start() if i + 1 < len(heads) else len(src)
        modules.setdefault(h.group(1), src[h.end():end])

def str_vars(body):
    return dict(re.findall(r'[\s,;{(](' + ID + r')="([a-z_.0-9]*\.)"', body))

classes = {}  # (module, local) -> dict
enums = {}  # (module, local) -> dict
exports = {}  # module -> {key: local}
imports = {}  # module -> {var: path}
for path, body in modules.items():
    if "static $()" not in body and ".QT)(" not in body:
        continue
    m = re.match(r'"use strict";r\.d\(e,\{(.*?)\}\)', body)
    exports[path] = dict(re.findall(r'(' + ID + r'):\(\)=>(' + ID + r')', m.group(1))) if m else {}
    imports[path] = dict(re.findall(r'(' + ID + r')=r\("([^"]+)"\)', body))
    svars = str_vars(body)
    bases = {}
    for cm in re.finditer(r'class (' + ID + r') extends (' + ID + r'(?:\.' + ID + r')?)\{', body):
        local, base = cm.group(1), cm.group(2)
        nxt = body.find("class ", cm.end())
        seg = body[cm.end(): nxt if nxt > 0 else len(body)]
        pm = re.search(r'static \$p\(\)\{return\s*(?:"([^"]*)"|(' + ID + r'))\}', seg)
        sm = re.search(r'static \$\(\)\{return\[("(?:[^"\\]|\\.)*")((?:,' + ID + r'(?:\.' + ID + r')?)*)\]\}', seg)
        bases[local] = (base, (pm.group(1) if pm.group(1) is not None else svars.get(pm.group(2))) if pm else None)
        if sm:
            classes[(path, local)] = {"spec": [json.loads(sm.group(1))], "refs": [r for r in sm.group(2).split(",") if r], "base": base}
    for local, info in classes.items():
        pass
    def pkg(local, seen=()):
        b, p = bases.get(local, (None, None))
        if p:
            return p
        if b and b in bases and b not in seen:
            return pkg(b, seen + (local,))
        return None
    for (mp, local), c in classes.items():
        if mp == path:
            c["pkg"] = pkg(local)
    for em in re.finditer(r'(' + ID + r')=\(0,' + ID + r'\.QT\)\(' + ID + r'\.' + ID + r',(' + ID + r'|"[^"]*"),"([^"]+)",(\[\[.*?\]\])(?:,(1|\[[^\]]*\]))?\)', body):
        local, p, name, vals, mode = em.groups()
        p = json.loads(p) if p.startswith('"') else svars.get(p)
        vals = json.loads(vals)
        short = name.rsplit(".", 1)[-1]
        if mode == "1":
            prefix = (short[0] + re.sub(r"[A-Z]", lambda x: "_" + x.group(0), short[1:])).upper() + "_"
            names = [prefix + v for _, v in vals]
        else:
            names = [v for _, v in vals]
        enums[(path, local)] = {"name": p + name, "values": [(n, no) for n, (no, _) in zip(names, vals)]}

def resolve(path, ref):
    if "." in ref:
        var, key = ref.split(".", 1)
        target = imports[path].get(var)
        if target and target.endswith("google/protobuf/struct_pb.js"):
            body = modules[target]
            local = dict(re.findall(r'(' + ID + r'):\(\)=>(' + ID + r')', body[:300])).get(key)
            names = {l: n for l, n in re.findall(r'(' + ID + r')\.typeName="(google\.protobuf\.\w+)"', body)}
            return ("wkt", names[local]) if local in names else None
        if target is None or target not in exports:
            return None
        return resolve(target, exports[target].get(key, key))
    if (path, ref) in classes:
        return ("msg", path, ref)
    if (path, ref) in enums:
        return ("enum", path, ref)
    return None

def full_name(path, local):
    c = classes[(path, local)]
    return c["pkg"] + c["spec"][0].split("|", 1)[0]

SCALAR = {1: "DOUBLE", 2: "FLOAT", 3: "INT64", 4: "UINT64", 5: "INT32", 6: "FIXED64", 7: "FIXED32",
          8: "BOOL", 9: "STRING", 12: "BYTES", 13: "UINT32", 15: "SFIXED32", 16: "SFIXED64", 17: "SINT32", 18: "SINT64"}

# Old descriptor: google.protobuf well-known types are copied verbatim (not compact specs).
old = open(old_text).read()
def old_message_block(name):
    m = re.search(r'\n  message_type \{\n    name: "' + re.escape(name) + r'"\n.*?\n  \}\n', old, re.S)
    return m.group(0) if m else None

msgs = collections.OrderedDict()  # full name -> (path, local)
enum_defs = collections.OrderedDict()
wkt = set()
by_name = {full_name(p, l): (p, l) for (p, l), c in classes.items() if c.get("pkg")}
missing = []
def visit(fn):
    if fn in msgs:
        return
    p, l = by_name[fn]
    msgs[fn] = (p, l)
    c = classes[(p, l)]
    for ref in c["refs"]:
        r = resolve(p, ref)
        if r is None:
            missing.append((fn, ref))
        elif r[0] == "wkt":
            continue
        elif r[0] == "msg":
            if not classes[(r[1], r[2])].get("pkg"):
                missing.append((fn, ref))
                continue
            visit(full_name(r[1], r[2]))
        else:
            enum_defs.setdefault(enums[(r[1], r[2])]["name"], enums[(r[1], r[2])])
for r in ROOTS:
    visit(r)

def field_type(path, refs, t, owner):
    if not t.startswith("#"):
        return ("TYPE_" + SCALAR[int(t)], None)
    r = resolve(path, refs[int(t[1:])])
    if r is None:
        return ("TYPE_MESSAGE", ".google.protobuf.UNRESOLVED")
    if r[0] == "wkt":
        return ("TYPE_MESSAGE", "." + r[1])
    if r[0] == "msg":
        c = classes[(r[1], r[2])]
        if not c.get("pkg"):
            return ("TYPE_MESSAGE", ".google.protobuf.UNRESOLVED")
        return ("TYPE_MESSAGE", "." + full_name(r[1], r[2]))
    return ("TYPE_ENUM", "." + enums[(r[1], r[2])]["name"])

def camel(s):
    out, up = "", False
    for ch in s:
        if ch == "_":
            up = True
        else:
            out += ch.upper() if up else ch
            up = False
    return out

def entry_name(field):
    return "".join(p[:1].upper() + p[1:] for p in field.split("_")) + "Entry"

def render_message(fn, path, local, indent, nested_msgs, nested_enums):
    c = classes[(path, local)]
    parts = c["spec"][0].split("|")
    short = parts[0].rsplit(".", 1)[-1]
    pad = " " * indent
    out = [pad + 'name: "%s"' % short]
    oneofs = []
    entries = []
    for spec in parts[1:]:
        tok = spec.split(" ")
        no, name, t = int(tok[0]), tok[1], tok[2]
        extra = tok[3] if len(tok) > 3 else None
        last = t[-1]
        if last in "?*":
            extra, t = last, t[:-1]
        f = [pad + "field {", pad + '  name: "%s"' % name, pad + "  number: %d" % no]
        if "," in t:
            k, v = t.split(",", 1)
            en = entry_name(name)
            entries.append((en, ("TYPE_" + SCALAR[int(k)], None), field_type(path, c["refs"], v, fn)))
            f += [pad + "  label: LABEL_REPEATED", pad + "  type: TYPE_MESSAGE", pad + '  type_name: ".%s.%s"' % (fn, en)]
        else:
            ty, tn = field_type(path, c["refs"], t, fn)
            f += [pad + "  label: %s" % ("LABEL_REPEATED" if extra == "*" else "LABEL_OPTIONAL"), pad + "  type: %s" % ty]
            if tn:
                f.append(pad + '  type_name: "%s"' % tn)
            if extra == "?":
                oneofs.append("_" + name)
                f.append(pad + "  oneof_index: %d" % (len(oneofs) - 1))
            elif extra not in (None, "*"):
                if extra not in oneofs:
                    oneofs.append(extra)
                f.append(pad + "  oneof_index: %d" % oneofs.index(extra))
        f.append(pad + '  json_name: "%s"' % camel(name))
        if extra == "?":
            f.append(pad + "  proto3_optional: true")
        f.append(pad + "}")
        out += f
    for nm, nested in nested_msgs:
        out.append(pad + "nested_type {")
        out += nested
        out.append(pad + "}")
    for en, kt, vt in entries:
        out.append(pad + "nested_type {")
        out.append(pad + '  name: "%s"' % en)
        for no, nm, (ty, tn) in ((1, "key", kt), (2, "value", vt)):
            out += [pad + "  field {", pad + '    name: "%s"' % nm, pad + "    number: %d" % no, pad + "    label: LABEL_OPTIONAL", pad + "    type: %s" % ty]
            if tn:
                out.append(pad + '    type_name: "%s"' % tn)
            out += [pad + '    json_name: "%s"' % nm, pad + "  }"]
        out += [pad + "  options {", pad + "    map_entry: true", pad + "  }", pad + "}"]
    for e in nested_enums:
        out.append(pad + "enum_type {")
        out += render_enum(e, indent + 2)
        out.append(pad + "}")
    for o in oneofs:
        out += [pad + "oneof_decl {", pad + '  name: "%s"' % o, pad + "}"]
    return out

def render_enum(e, indent):
    pad = " " * indent
    out = [pad + 'name: "%s"' % e["name"].rsplit(".", 1)[-1]]
    for n, no in e["values"]:
        out += [pad + "value {", pad + '  name: "%s"' % n, pad + "  number: %d" % no, pad + "}"]
    return out

# Build nesting: message "A.B" nests under "A" (same package).
def pkg_of(fn):
    return classes[msgs[fn]]["pkg"].rstrip(".") if fn in msgs else None

all_names = set(msgs)
def parent_of(fn, pkg):
    rel = fn[len(pkg) + 1:]
    if "." not in rel:
        return None
    return pkg + "." + rel.rsplit(".", 1)[0]

def build(fn, indent):
    pkg = pkg_of(fn)
    kids = [k for k in msgs if pkg_of(k) == pkg and parent_of(k, pkg) == fn]
    kid_enums = [e for n, e in enum_defs.items() if n.startswith(pkg + ".") and parent_of(n, pkg) == fn]
    nested = [(k, build(k, indent + 2)) for k in kids]
    p, l = msgs[fn]
    return render_message(fn, p, l, indent, nested, kid_enums)

files = collections.OrderedDict((("aiserver.v1", []), ("google.protobuf", []), ("agent.v1", [])))
for fn in msgs:
    pkg = pkg_of(fn)
    if parent_of(fn, pkg) is None:
        files.setdefault(pkg, []).append(fn)
used_wkt = set()
for fn in msgs:
    for line in build(fn, 0):
        m = re.search(r'type_name: "\.google\.protobuf\.(\w+)', line)
        if m:
            used_wkt.add(m.group(1))
out = []
deps = {"agent.v1": ["aiserver_v1.proto", "google_protobuf.proto"]}
for pkg, names in files.items():
    out.append("file {")
    out.append('  name: "%s.proto"' % pkg.replace(".", "_"))
    out.append('  package: "%s"' % pkg)
    for d in deps.get(pkg, []):
        out.append('  dependency: "%s"' % d)
    if pkg == "google.protobuf":
        for w in ["Value", "Struct", "ListValue"]:
            out.append(old_message_block(w).strip("\n"))
        m = re.search(r'\n  enum_type \{\n    name: "NullValue"\n.*?\n  \}\n', old, re.S)
        out.append(m.group(0).strip("\n"))
    for fn in names:
        out.append("  message_type {")
        out += build(fn, 4)
        out.append("  }")
    for n, e in enum_defs.items():
        if n.startswith(pkg + ".") and parent_of(n, pkg) is None:
            out.append("  enum_type {")
            out += render_enum(e, 4)
            out.append("  }")
    out.append('  syntax: "proto3"')
    out.append("}")
print("\n".join(out))
sys.stderr.write("messages %d enums %d unresolved %d wkt %s\n" % (len(msgs), len(enum_defs), len(missing), sorted(used_wkt)))
for fn, ref in missing[:20]:
    sys.stderr.write("  unresolved %s %s\n" % (fn, ref))
