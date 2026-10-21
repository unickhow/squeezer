import type { AppPreferences, CompressionOptions, OutputLocation, VideoOptions } from "../types";

export const defaultPreferences: AppPreferences = {
  schemaVersion: 1,
  outputLocation: { kind: "sourceFolder" },
  image: { mode: "quality", minSavingsPercent: 10 },
  video: { quality: "balanced", maxDimension: 0, targetBytes: null, minSavingsPercent: 10 },
};

export function imageOptionsFromPreferences(value: AppPreferences): CompressionOptions {
  return { mode: value.image.mode, outputFormat: "original", preserveMetadata: true, minSavingsPercent: value.image.minSavingsPercent };
}

export function videoOptionsFromPreferences(value: AppPreferences): VideoOptions {
  return { quality: value.video.quality, maxDimension: value.video.maxDimension, targetBytes: value.video.targetBytes ?? undefined, minSavingsPercent: value.video.minSavingsPercent };
}

export function resolveBatchOutputDirectory(location: OutputLocation, pickedFolder?: string | null): { cancelled: true } | { cancelled: false; directory?: string } {
  if (location.kind === "sourceFolder") return { cancelled: false };
  if (location.kind === "fixedFolder") return { cancelled: false, directory: location.path };
  return pickedFolder ? { cancelled: false, directory: pickedFolder } : { cancelled: true };
}

export async function runBatchWithOutputLocation(
  location: OutputLocation,
  pickFolder: () => Promise<string | null>,
  start: (directory?: string) => Promise<void>,
): Promise<{ cancelled: boolean }> {
  const pickedFolder = location.kind === "askEachBatch" ? await pickFolder() : undefined;
  const destination = resolveBatchOutputDirectory(location, pickedFolder);
  if (destination.cancelled) return { cancelled: true };
  await start(destination.directory);
  return { cancelled: false };
}

export function createPreferenceSaver(
  persist: (value: AppPreferences) => Promise<void>,
  onSaved: (value: AppPreferences) => void,
) {
  let current: AppPreferences | null = null;
  let queue = Promise.resolve();
  return {
    setCurrent(value: AppPreferences) { current = value; },
    save(change: AppPreferences | ((value: AppPreferences) => AppPreferences)) {
      const operation = queue.then(async () => {
        if (!current && typeof change === "function") throw new Error("偏好設定尚未載入");
        const next = typeof change === "function" ? change(current!) : change;
        await persist(next);
        current = next;
        onSaved(next);
      });
      queue = operation.catch(() => undefined);
      return operation;
    },
  };
}
