# ycallr CLI

Terminal runner for [ycallr-core](https://github.com/Coditary/ycallr-core): YAML API profiles are compiled to protobuf and invoked via FFI.

```bash
ycallr install ~/.config/ycallr/apis/github.yaml
ycallr github --tree
ycallr github create-issue --owner=rust-lang --repo=rust --title=Bug
```

See the ycallr-core repository for profile format and engine documentation.
