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

// The url / finalUrl / referrer values below are what Chromium reported for these cases when the
// real extension ran against stand-in hosts (docs/verification.md). A download that goes through
// BOOTH's redirect carries only the origin of the page as its referrer.
const signed = (id, name) => `https://files.example.test/signed/${id}/${name}?sig=abc`;

test("ordinary BOOTH downloads are decided as before", () => {
  // No product ID anywhere: left alone. The click record, not this fallback, routes these.
  for (const referrer of ["https://booth.pm/", "https://shop.booth.pm/", "https://accounts.booth.pm/"]) {
    assert.equal(
      decideFilename(
        { filename: "body.zip", url: "https://booth.pm/downloadables/1001", finalUrl: signed(1001, "body.zip"), referrer },
        true,
      ),
      null,
      referrer,
    );
  }
  // BOOTH's own redirect ends at a path that names the product.
  assert.deepEqual(
    decideFilename(
      {
        filename: "body.zip",
        url: "https://booth.pm/downloadables/1008",
        finalUrl: "https://files.example.test/items/424248/body.zip?sig=abc",
        referrer: "https://accounts.booth.pm/",
      },
      true,
    ),
    { filename: "VRCLibMan/424248/body.zip", conflictAction: "uniquify" },
  );
  // A BOOTH product page that linked to the file, when the browser reports its full address.
  assert.deepEqual(
    decideFilename(
      { filename: "body.vrm", url: "https://booth.pm/downloadables/1001", referrer: "https://shop.booth.pm/items/424243?utm_source=x" },
      true,
    ),
    { filename: "VRCLibMan/424243/body.vrm", conflictAction: "uniquify" },
  );
});

test("another site cannot choose the product a file is filed under", () => {
  const cases = {
    "a page elsewhere whose address names a product, linking to a BOOTH file": {
      filename: "body.unitypackage",
      url: "https://booth.pm/downloadables/2001",
      finalUrl: signed(2001, "body.unitypackage"),
      referrer: "https://attacker.example.test/items/5058077?unsafe=1",
    },
    "a BOOTH page linking to a file on another site": {
      filename: "body.unitypackage",
      url: "https://attacker.example.test/items/5058077/body.unitypackage",
      finalUrl: "https://attacker.example.test/items/5058077/body.unitypackage",
      referrer: "https://booth.pm/",
    },
    "a product named in the query string of a BOOTH download link": {
      filename: "body.zip",
      url: "https://booth.pm/downloadables/2003?x=/items/5058077",
      finalUrl: signed(2003, "body.zip"),
      referrer: "https://attacker.example.test/",
    },
    "a product named in the query string of a BOOTH page": {
      filename: "body.zip",
      url: "https://booth.pm/downloadables/2004",
      finalUrl: signed(2004, "body.zip"),
      referrer: "https://booth.pm/ja/search/x?q=/items/5058077",
    },
    "a look-alike host": {
      filename: "body.zip",
      url: "https://booth.pm.example.test/items/5058077/body.zip",
      referrer: "https://evilbooth.pm/items/5058077",
    },
    "a BOOTH address over http or with a port": {
      filename: "body.zip",
      url: "http://booth.pm/items/5058077/body.zip",
      referrer: "https://booth.pm:8443/items/5058077",
    },
  };
  for (const [name, item] of Object.entries(cases)) assert.equal(decideFilename(item, true), null, name);
});

test("a finished file keeps the product of the folder it was staged in", () => {
  const staged = "C:\\Users\\kei\\Downloads\\VRCLibMan\\424244\\body.unitypackage";
  const base = { filename: staged, url: "https://booth.pm/downloadables/1004", finalUrl: signed(1004, "body.unitypackage"), fileSize: 12 };
  for (const referrer of ["https://accounts.booth.pm/", "https://booth.pm/", "", "https://attacker.example.test/items/5058077"]) {
    assert.equal(completionNotice({ ...base, referrer }).boothId, "424244", referrer);
  }
  assert.equal(
    completionNotice({ ...base, url: "https://booth.pm/downloadables/1004?x=/items/5058077" }).boothId,
    "424244",
  );
  assert.equal(
    completionNotice({ ...base, filename: "/home/kei/Downloads/VRCLibMan/424244/body.unitypackage" }).path,
    "/home/kei/Downloads/VRCLibMan/424244/body.unitypackage",
  );
});
