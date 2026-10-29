# Model Plane Protobuf

`baseline.binpb` is a buf image (`FileDescriptorSet`) representing the locked-in
public API of the Model Plane v2 protos. CI runs `buf breaking` against this
file to prevent backwards-incompatible changes.

To regenerate after an intentional, reviewed breaking change:

```bash
buf build "apps/Model Plane/proto" -o "apps/Model Plane/proto/baseline.binpb"
```
