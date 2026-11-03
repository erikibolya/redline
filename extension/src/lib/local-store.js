/**
 * Local (offline) captures. Blobs stay in IndexedDB in this browser profile; nothing is sent to a
 * RedLine server. The side panel exports a folder/ZIP for the human to hand to an agent.
 */
const DB_NAME = "redline-local";
const STORE = "captures";

function openDb() {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(DB_NAME, 1);
    request.onupgradeneeded = () => {
      if (!request.result.objectStoreNames.contains(STORE)) {
        request.result.createObjectStore(STORE, { keyPath: "id" });
      }
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error || new Error("cannot open local store"));
  });
}

function transaction(mode, action) {
  return openDb().then(
    (db) =>
      new Promise((resolve, reject) => {
        const tx = db.transaction(STORE, mode);
        let result;
        try {
          result = action(tx.objectStore(STORE));
        } catch (error) {
          reject(error);
          return;
        }
        tx.oncomplete = () => resolve(result);
        tx.onerror = () => reject(tx.error || new Error("local store transaction failed"));
        tx.onabort = () => reject(tx.error || new Error("local store transaction aborted"));
      }),
  );
}

export function putCapture(capture) {
  return transaction("readwrite", (store) => {
    store.put(capture);
    return capture;
  });
}

export function getCapture(id) {
  return openDb().then(
    (db) =>
      new Promise((resolve, reject) => {
        const request = db.transaction(STORE, "readonly").objectStore(STORE).get(id);
        request.onsuccess = () => resolve(request.result || null);
        request.onerror = () => reject(request.error || new Error("cannot read local capture"));
      }),
  );
}

export function deleteCapture(id) {
  return transaction("readwrite", (store) => {
    store.delete(id);
  });
}

export async function updateCapture(id, patch) {
  const current = await getCapture(id);
  if (!current) return null;
  return putCapture({ ...current, ...patch });
}

export function newLocalId(prefix = "local") {
  return `${prefix}-${Date.now()}-${Math.random().toString(16).slice(2, 8)}`;
}
