import { synthesizeSpeechStream, type SpeechOptions } from "./commands";

export interface StreamingSpeech {
  /** Stops synthesis playback early; `done` resolves. */
  stop(): void;
  /** Resolves when playback finishes; rejects when synthesis or playback fails. */
  done: Promise<void>;
}

/**
 * Plays `text` through Media Source Extensions, so the first words are heard while the rest of the
 * utterance is still being synthesized. Both providers return MP3, which Chromium plays from a
 * `audio/mpeg` SourceBuffer.
 */
export function playStreamingSpeech(
  text: string,
  options: SpeechOptions | undefined,
): StreamingSpeech {
  let audio: HTMLAudioElement | null = null;
  let mediaSource: MediaSource | null = null;
  let sourceBuffer: SourceBuffer | null = null;
  let objectUrl: string | null = null;
  const queue: ArrayBuffer[] = [];
  let finished = false;
  let stopped = false;
  let resolveDone!: () => void;
  let rejectDone!: (error: unknown) => void;
  const done = new Promise<void>((resolve, reject) => {
    resolveDone = resolve;
    rejectDone = reject;
  });

  function flush() {
    if (stopped || !sourceBuffer || sourceBuffer.updating) return;
    const chunk = queue.shift();
    if (chunk) {
      try {
        sourceBuffer.appendBuffer(chunk);
      } catch (error) {
        rejectDone(error);
      }
      return;
    }
    if (finished && mediaSource && mediaSource.readyState === "open") {
      try {
        mediaSource.endOfStream();
      } catch {
        // The stream was already ended.
      }
    }
  }

  function startPlayer(contentType: string) {
    if (audio || stopped) return;
    mediaSource = new MediaSource();
    objectUrl = URL.createObjectURL(mediaSource);
    audio = new Audio(objectUrl);
    audio.onended = () => resolveDone();
    audio.onerror = () => rejectDone(new Error("语音播放失败，请重试或检查语音服务设置"));
    mediaSource.addEventListener(
      "sourceopen",
      () => {
        if (!mediaSource || stopped) return;
        sourceBuffer = mediaSource.addSourceBuffer(contentType);
        sourceBuffer.mode = "sequence";
        sourceBuffer.addEventListener("updateend", flush);
        flush();
      },
      { once: true },
    );
    void audio.play().catch((error) => rejectDone(error));
  }

  void synthesizeSpeechStream(text, options, (event) => {
    if (stopped) return;
    if (event.type === "start") {
      startPlayer(event.contentType);
    } else {
      queue.push(base64ToBytes(event.data));
      flush();
    }
  })
    .then(() => {
      finished = true;
      if (!stopped) flush();
    })
    .catch((error) => {
      if (!stopped) rejectDone(error);
    });

  return {
    stop() {
      if (stopped) return;
      stopped = true;
      queue.length = 0;
      if (audio) {
        audio.onended = null;
        audio.onerror = null;
        audio.pause();
        audio.src = "";
      }
      if (objectUrl) URL.revokeObjectURL(objectUrl);
      objectUrl = null;
      resolveDone();
    },
    done,
  };
}

function base64ToBytes(value: string): ArrayBuffer {
  const binary = atob(value);
  const buffer = new ArrayBuffer(binary.length);
  const bytes = new Uint8Array(buffer);
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index);
  }
  return buffer;
}
