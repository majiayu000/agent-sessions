# Cursor Agent CLI protobuf descriptor

`src/import/protocols/cursor-agent-2026.10.22.pb` is rebuilt from the installed
Cursor Agent CLI bundle; the repository ships no vendor `.proto` files. Both
scripts only parse local text and write to stdout.

1. `gen_descriptor.py` reads the bundle's compact protobuf-es specs and emits the
   transitive closure of the conversation-store root messages as a
   `FileDescriptorSet` text proto. The previous descriptor supplies the
   `google.protobuf` well-known types.
2. `merge.py` keeps definitions that upstream removed (messages, fields, enum
   values) so older stores still decode, and aborts if a number is reused with a
   different definition.

```sh
INC=/opt/homebrew/include          # directory holding google/protobuf/descriptor.proto
OLD=src/import/protocols/cursor-agent-2026.10.01.pb   # previous descriptor (from git history)
BUNDLE=~/.local/share/cursor-agent/versions/2026.10.22-27dcf5f
protoc -I$INC --decode=google.protobuf.FileDescriptorSet google/protobuf/descriptor.proto < $OLD > old.txt
python3 -I scripts/cursor-agent-descriptor/gen_descriptor.py "$BUNDLE" old.txt > new.txt
python3 -I scripts/cursor-agent-descriptor/merge.py old.txt new.txt > merged.txt
protoc -I$INC --encode=google.protobuf.FileDescriptorSet google/protobuf/descriptor.proto < merged.txt > cursor-agent-NEW.pb
```

Run from a scratch directory; for 2026.10.22-27dcf5f with libprotoc 35.1 the
output is byte-identical to the committed file (SHA-1 `16f083a2…`).
