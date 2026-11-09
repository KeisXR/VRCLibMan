import assert from "node:assert/strict";
import { createRequire } from "node:module";
import test from "node:test";

const require = createRequire(import.meta.url);
const { decideFilename, completionNotice } = require("../extension/download-route.js");

test("routing stays off until the setting is on, and only booth assets with an id move", () => {
  const item = {
    filename: "body.unitypackage",
    url: "https://booth.pm/downloads/424242/file",
    referrer: "https://booth.pm/ja/items/424242",
  };
  assert.equal(decideFilename(item, false), null);
  assert.equal(decideFilename(item, undefined), null);
  assert.deepEqual(decideFilename(item, true), {
    filename: "VRCLibMan/424242/body.unitypackage",
    conflictAction: "uniquify",
  });
  assert.equal(
    decideFilename({ ...item, filename: "notes.pdf" }, true),
    null,
  );
  assert.equal(
    decideFilename(
      { filename: "body.zip", url: "https://example.test/body.zip", referrer: "https://example.test/" },
      true,
    ),
    null,
  );
  assert.equal(
    decideFilename({ filename: "body.zip", url: "https://booth.pm/files/body.zip", referrer: "" }, true),
    null,
  );
});

test("a finished staged file can be registered and other downloads cannot", () => {
  assert.equal(completionNotice({ filename: "C:\\Users\\kei\\Downloads\\notes.json", state: "complete" }), null);
  assert.deepEqual(
    completionNotice({
      filename: "C:\\Users\\kei\\Downloads\\VRCLibMan\\424242\\body.unitypackage",
      url: "https://booth.pm/downloads/424242/file",
      referrer: "https://booth.pm/ja/items/424242",
      fileSize: 12,
    }),
    {
      boothId: "424242",
      filename: "body.unitypackage",
      path: "C:\\Users\\kei\\Downloads\\VRCLibMan\\424242\\body.unitypackage",
      size: 12,
      variation: "",
    },
  );
});
