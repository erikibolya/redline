import test from "node:test";
import assert from "node:assert/strict";
import {
  compareVersions,
  planUpdate,
  submittedFromStatus,
  versionFromRevision,
} from "../scripts/cws.mjs";

test("extension versions compare numerically, not as strings", () => {
  assert.equal(compareVersions("0.7.3", "0.7.2"), 1);
  assert.equal(compareVersions("0.7.10", "0.7.9"), 1);
  assert.equal(compareVersions("0.7.3", "0.7.3"), 0);
  assert.equal(compareVersions("0.8", "0.7.99"), 1);
});

test("the published version is read from the distribution channels", () => {
  assert.equal(
    versionFromRevision({
      state: "PUBLISHED",
      distributionChannels: [{ crxVersion: "0.7.2" }, { crxVersion: "0.7.3" }],
    }),
    "0.7.3",
  );
  assert.equal(versionFromRevision({ state: "PUBLISHED" }), null);
});

test("a submission carries its review state", () => {
  assert.deepEqual(
    submittedFromStatus({
      submittedItemRevisionStatus: {
        state: "PENDING_REVIEW",
        distributionChannels: [{ crxVersion: "0.7.3" }],
      },
    }),
    { version: "0.7.3", state: "PENDING_REVIEW" },
  );
  assert.equal(submittedFromStatus({}), null);
});

test("a published version needs no upload", () => {
  assert.deepEqual(planUpdate({ manifestVersion: "0.7.3", published: "0.7.3", submitted: null }), {
    action: "skip",
    reason: "0.7.3 is already published",
  });
});

test("a newer manifest is uploaded and submitted", () => {
  assert.equal(
    planUpdate({ manifestVersion: "0.7.3", published: "0.7.2", submitted: null }).action,
    "upload",
  );
});

test("a version already in review is not uploaded twice", () => {
  assert.equal(
    planUpdate({
      manifestVersion: "0.7.3",
      published: "0.7.2",
      submitted: { version: "0.7.3", state: "PENDING_REVIEW" },
    }).action,
    "skip",
  );
});

test("a rejected submission is retried, and a newer submission is an error", () => {
  assert.equal(
    planUpdate({
      manifestVersion: "0.7.3",
      published: "0.7.2",
      submitted: { version: "0.7.3", state: "REJECTED" },
    }).action,
    "upload",
  );
  assert.equal(
    planUpdate({
      manifestVersion: "0.7.3",
      published: "0.7.2",
      submitted: { version: "0.7.4", state: "PENDING_REVIEW" },
    }).action,
    "error",
  );
});

test("a store version ahead of the manifest is an error, never a downgrade", () => {
  assert.equal(
    planUpdate({ manifestVersion: "0.7.3", published: "0.7.4", submitted: null }).action,
    "error",
  );
});
