# `claw patch`

Create, apply, inspect, and reason about codec-aware patch objects.

## Create And Apply

```bash
claw patch codecs
claw patch --json codecs --path api/openapi.yaml
claw patch create --old before.json --new after.json --path config.json
claw patch create --old before.toml --new after.toml --path Claw.toml
claw patch create --old base.yaml --new next.yaml --path deploy/k8s/service.yaml
claw patch show clw_...
claw patch apply --patch clw_... --file before.json
```

`create` selects the codec from the target path extension and stores the patch
as a Claw object. Use `--json` for machine-readable patch IDs and operations.
The default registry includes structural codecs for JSON, TOML, YAML,
OpenAPI/Swagger specs, Kubernetes manifests, Jupyter notebooks, Rust,
TypeScript/JavaScript, Python, SQL migrations, Protobuf, and Terraform.
Source and infrastructure codecs operate on top-level semantic items and emit
canonical text when applying operations.

`codecs` lists the default codec registry, including stable codec IDs,
extension routes, path matchers, families, operation models, canonical-output
behavior, and the fallback binary codec. Operation models are scriptable labels
such as `rust_top_level_ast_items`, `typescript_top_level_declarations`,
`python_top_level_definitions`, `sql_statements`, `protobuf_declarations`,
`terraform_hcl_blocks`, `openapi_spec_tree`, `kubernetes_manifest_tree`, and
`notebook_json_tree`. Use `--path <repo-path>` to resolve the exact codec that
`create` and `merge3` would use for a file before building patches.

## Patch Algebra Workbench

Use the workbench commands to explain whether patches can be reordered, undone,
or merged by the selected semantic codec.

```bash
claw patch commute --left clw_... --right clw_...
claw patch invert --patch clw_...
claw patch merge3 --base base.rs --left left.rs --right right.rs --path src/lib.rs --out merged.rs
claw patch workbench --left clw_... --right clw_...
```

`--json` emits schema version `1` with namespaced actions:
`patch.codecs`, `patch.create`, `patch.apply`, `patch.show`, `patch.commute`,
`patch.invert`, `patch.merge3`, or `patch.workbench`. `commute --json` prints the reordered
operation streams when the codec accepts the reorder, or a reason when the
patches conflict. `apply --json` reports the patch summary, file path, codec,
and bytes written after updating the file. `invert --json` prints undo
operations. `merge3` reads three files, selects the codec from `--path`, and
writes the merged content to `--out` or stdout. `workbench --json` combines the
algebra checks into one receipt with a classification, commute reason, reorder
payload when available, structured operation analysis, invertibility for each
side, and human-readable reasons. The `analysis` object reports whether the
patches share a path, codec, base object, and result object; lists unique
operation addresses for each side; identifies overlapping addresses; and gives
scriptable `decision`, `address_relation`, `reorder_available`, and
`left_invertible`/`right_invertible` fields.

Patch commands fail closed on unsupported codecs, invalid structured documents
for tree codecs, and non-commutable patch streams.
