# Docs for AI assistants

The documentation is also available in forms that AI coding assistants can read. They show the
latest release.

## llms.txt

- [`https://fagus.niklasforst.de/llms.txt`](https://fagus.niklasforst.de/llms.txt): a list of all
  pages with a short description and a link to the Markdown of each.
- [`https://fagus.niklasforst.de/llms-full.txt`](https://fagus.niklasforst.de/llms-full.txt): all
  pages as Markdown in one file.

Every page is also available as Markdown: add `.md` to its address, for example
`https://fagus.niklasforst.de/docs/getting-started.md`.

## MCP server

`https://fagus.niklasforst.de/docs/mcp` is an MCP server over Streamable HTTP, without
authentication. Its tools search the documentation (`search_docs`), read a page or one of its
sections (`get_page`) and list the pages (`list_pages`).

In Claude Code:

```sh
claude mcp add --transport http fagus-docs https://fagus.niklasforst.de/docs/mcp
```

In other clients, add `https://fagus.niklasforst.de/docs/mcp` as a remote MCP server over HTTP.
