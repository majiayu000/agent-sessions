"""Union an older descriptor text proto into a newer one.

Definitions removed upstream (whole messages/enums, fields, enum values) are retained from the
older descriptor so older stores keep decoding; a number reused with a different definition aborts.
usage: python3 -I merge.py old.txt new.txt > merged.txt
"""
import sys


def parse(text):
    lines = text.split("\n")
    pos = 0

    def block():
        nonlocal pos
        items = []
        while pos < len(lines):
            line = lines[pos].strip()
            pos += 1
            if not line:
                continue
            if line == "}":
                return items
            if line.endswith("{"):
                items.append((line[:-1].strip(), block()))
            else:
                k, v = line.split(":", 1)
                items.append((k.strip(), v.strip()))
        return items

    return block()


def dump(items, indent=0):
    out = []
    for k, v in items:
        if isinstance(v, list):
            out.append(" " * indent + k + " {")
            out += dump(v, indent + 2)
            out.append(" " * indent + "}")
        else:
            out.append(" " * indent + "%s: %s" % (k, v))
    return out


def get(items, key):
    return next((v for k, v in items if k == key), None)


def children(items, key):
    return [v for k, v in items if k == key]


added = []


def merge_enum(old, new, path):
    have = {get(v, "number"): get(v, "name") for v in children(new, "value")}
    for v in children(old, "value"):
        num, name = get(v, "number"), get(v, "name")
        if num not in have:
            new.append(("value", v))
            added.append("enum value %s.%s" % (path, name))
        elif have[num] != name:
            sys.exit("enum number conflict %s %s" % (path, num))


def oneof_names(msg):
    return [get(o, "name") for o in children(msg, "oneof_decl")]


def merge_message(old, new, path):
    new_fields = {get(f, "number"): f for f in children(new, "field")}
    old_oneofs, new_oneofs = oneof_names(old), oneof_names(new)
    for f in children(old, "field"):
        num = get(f, "number")
        if num in new_fields:
            a = [(k, v) for k, v in f if k != "oneof_index"]
            b = [(k, v) for k, v in new_fields[num] if k != "oneof_index"]
            if a != b:
                sys.exit("field conflict %s %s" % (path, num))
            continue
        f = list(f)
        oi = get(f, "oneof_index")
        if oi is not None:
            name = old_oneofs[int(oi)]
            if name not in new_oneofs:
                new_oneofs.append(name)
                # oneof_decl entries must stay after existing ones to keep indices stable
                new.append(("oneof_decl", [("name", name)]))
            f = [(k, str(new_oneofs.index(name)) if k == "oneof_index" else v) for k, v in f]
        # insert after the last field to keep field blocks together
        last = max(i for i, (k, _) in enumerate(new) if k == "field")
        new.insert(last + 1, ("field", f))
        added.append("field %s.%s" % (path, get(f, "name")))
    merge_children(old, new, path)


def merge_children(old, new, path):
    for key, fn in (("nested_type", merge_message), ("message_type", merge_message), ("enum_type", merge_enum)):
        new_by = {get(v, "name"): v for v in children(new, key)}
        for v in children(old, key):
            name = get(v, "name")
            child = "%s.%s" % (path, name.strip('"'))
            if name in new_by:
                fn(v, new_by[name], child)
            else:
                # place before oneof_decl/options to keep text proto order conventional
                idx = next((i for i, (k, _) in enumerate(new) if k in ("syntax",)), len(new))
                new.insert(idx, (key, v))
                added.append("%s %s" % (key, child))


old = parse(open(sys.argv[1]).read())
new = parse(open(sys.argv[2]).read())
new_files = {get(f, "name"): f for f in children(new, "file")}
for f in children(old, "file"):
    merge_children(f, new_files[get(f, "name")], get(f, "package").strip('"'))
print("\n".join(dump(new)))
for a in added:
    sys.stderr.write("retained " + a + "\n")
