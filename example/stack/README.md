# Example quadlet stack

A Stack is a directory in git holding quadlet unit files. Point a Stack
resource at it with `run_directory = "example/stack"` and it deploys to
`/etc/containers/systemd/<stack>/` on the Server.

- `[[DOMAIN]]` is a Variable, resolved by Core and written into the files.
- `[[DB_PASSWORD]]` is a secret. It may only appear on a `Secret=` line of a
  unit file. Core resolves it from the Stack's `secrets_collection` and the
  agent creates the podman secret `<stack>_DB_PASSWORD`.
- `config.yml` contains a secret reference, so it is mounted with
  `Secret=file:config.yml` and becomes the podman secret `<stack>_config.yml`.
  It is never written under `/etc`.
- `app.pod` gets a fixed user namespace (`UIDMap=` / `GIDMap=` on a slice
  the agent picks once per host and keeps); `web` and `db` share it as pod
  members. A `.container` outside a pod gets its own slice. Set `UserNS=`
  yourself (`host`, `keep-id`, ...) to opt out.
