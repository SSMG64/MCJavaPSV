# Packaging

`horizon-rev.txt` pins the Horizon revision that CI builds `horizon-prepare` from and that
`crates/psv-runtime` depends on. Change it in one commit together with the `rev` values in
`crates/psv-runtime/Cargo.toml`, and rebuild everything before merging.

A release is a zip with this layout:

```
mcjavapsv.exe
mcjavapsv-cli.exe
payload/
  horizon-prepare.exe
  research-lock.json
  HORIZON-LICENSE
  eboot.bin
docs/
LICENSE
SHA256SUMS
```
