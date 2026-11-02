import assert from "node:assert/strict";
import test from "node:test";
import { createServer } from "vite";

async function withPrompt(run) {
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  try {
    await run(await server.ssrLoadModule("/src/browserAutoPrompt.ts"));
  } finally {
    await server.close();
  }
}

test("postponing the shown products keeps them postponed until a new one arrives", async () => {
  await withPrompt((prompt) => {
    const shown = ["345678", "456789"];
    const postponed = prompt.postponeShown([], shown);
    assert.deepEqual(postponed, shown);
    assert.equal(prompt.promptOpen(shown, postponed), false);
    assert.equal(prompt.showsResume(shown, postponed), true);
    assert.equal(prompt.reconcilePostponed(postponed, shown), postponed, "the same pending list stays postponed");

    const added = [...shown, "567890"];
    const reopened = prompt.reconcilePostponed(postponed, added);
    assert.deepEqual(reopened, []);
    assert.equal(prompt.promptOpen(added, reopened), true);
    assert.equal(prompt.showsResume(added, reopened), false);

    const resumed = [];
    assert.equal(prompt.promptOpen(shown, resumed), true, "the resume button clears the postponement");
  });
});

test("postpone and reconcile keep the same array when nothing changed", async () => {
  await withPrompt((prompt) => {
    const postponed = ["345678", "456789"];
    const empty = [];
    assert.equal(prompt.postponeShown(postponed, postponed), postponed);
    assert.equal(prompt.reconcilePostponed(postponed, postponed), postponed);
    assert.equal(prompt.reconcilePostponed(empty, ["345678"]), empty);
  });
});

test("a postponed id that left the list is dropped, and an empty queue shows nothing", async () => {
  await withPrompt((prompt) => {
    const left = prompt.reconcilePostponed(["345678", "456789"], ["456789"]);
    assert.deepEqual(left, ["456789"]);
    assert.equal(prompt.showsResume(["456789"], left), true);
    assert.deepEqual(prompt.reconcilePostponed(["456789"], ["456789", "567890"]), []);
    assert.equal(prompt.promptOpen([], ["456789"]), false);
    assert.equal(prompt.showsResume([], ["456789"]), false);
    assert.deepEqual(prompt.postponeShown(["456789"], []), []);
  });
});
