Good documentation gives an idea room to breathe. This page is a small specimen of the **shared reading style** used throughout the docs and journal: comfortable line lengths, clear headings and useful details.

## Start with a clear idea

Write the smallest explanation that makes the next step obvious. Use *emphasis* for a distinction, **bold** for something worth remembering, and `inline code` for an exact identifier.

Follow [the architectural field note](/blog/one-engine/) to see the same prose styling in a blog post, or [return to the overview](/docs/).

### Give each detail a place

- Keep one source of analysis state.
- Expose that state through the interfaces that need it.
- Make freshness explicit rather than leaving consumers to guess.
  - A generation identifies the state behind a result.
  - Cancellation lets callers stop work they no longer need.

#### A short sequence

1. Open the project and read its configuration.
2. Build a snapshot of the current state.
3. Share the result with the editor and assistant.

> A useful abstraction makes the important thing visible.
>
> Give readers the context they need, then let them get back to their work.

## Show the working parts

Code keeps its whitespace and scrolls inside its own panel on narrow screens.

```sh
make website-build
make website-run
```

<div class="table-scroll" role="region" aria-label="Interface comparison" tabindex="0">
<table aria-label="Shared engine interfaces">
  <thead><tr><th scope="col">Surface</th><th scope="col">Reader</th><th scope="col">Shared foundation</th></tr></thead>
  <tbody>
    <tr><th scope="row">LSP</th><td>Your editor</td><td>Analysis state and document changes</td></tr>
    <tr><th scope="row">MCP</th><td>Your assistant</td><td>The same engine and generation information</td></tr>
  </tbody>
</table>
</div>

<figure>
  <img src="/assets/images/engine.svg" width="1440" height="600" alt="One orange analysis engine connects to two dark terminal surfaces.">
  <figcaption>One shared engine, two interfaces. The image scales with the reading column.</figcaption>
</figure>

---

## Leave the extra detail within reach

<details>
<summary>How is this example styled?</summary>

The docs, journal article and standalone prose page all use the same `.prose` class and site stylesheet. The layout changes; the reading experience stays consistent.

</details>

For a different context, [read the standalone prose page](/prose/) or [open this specimen inside the docs](/docs/prose/).
