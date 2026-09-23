# komodo_client (Quomodo)

TypeScript client for the Quomodo API. Generated from the Rust types with typeshare; the package keeps the upstream name so the UI imports do not change.

```sh
npm install komodo_client
```

or

```sh
yarn add komodo_client
```

```ts
import { KomodoClient, Types } from "komodo_client";

const komodo = KomodoClient("https://demo.komo.do", {
  type: "api-key",
  params: {
    key: "your_key",
    secret: "your secret",
  },
});

// Inferred as Types.StackListItem[]
const stacks = await komodo.read("ListStacks", {});

// Inferred as Types.Stack
const stack = await komodo.read("GetStack", {
  stack: stacks[0].name,
});
```
