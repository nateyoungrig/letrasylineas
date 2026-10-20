"""Transcribes a vocals track with faster-whisper and prints one JSON object per line.

Run by the app as:  uv run --python 3.11 --with faster-whisper transcribe.py VOCALS LANG PROMPT
Events: {"event": "info"|"segment"|"reset"|"log", ...}. GPU first, CPU if CUDA is unusable.
"""

import glob
import json
import os
import site
import sys


def use_pip_cuda_libraries():
    """Points the dynamic linker at the CUDA libraries from the nvidia-* pip packages.

    The linker reads LD_LIBRARY_PATH only at startup, so the script restarts itself once.
    """
    # uv puts --with packages in an overlay that is on sys.path but not in site.getsitepackages().
    roots = dict.fromkeys(sys.path + site.getsitepackages())
    dirs = [d for root in roots if root for d in sorted(glob.glob(os.path.join(root, "nvidia", "*", "lib")))]
    current = os.environ.get("LD_LIBRARY_PATH", "")
    missing = [d for d in dirs if d not in current.split(":")]
    if missing and not os.environ.get("LETRAS_CUDA_REEXEC"):
        os.environ["LD_LIBRARY_PATH"] = ":".join(missing + ([current] if current else []))
        os.environ["LETRAS_CUDA_REEXEC"] = "1"
        os.execv(sys.executable, [sys.executable] + sys.argv)


use_pip_cuda_libraries()

from faster_whisper import WhisperModel  # noqa: E402

MODEL = "large-v3"


def emit(**event):
    print(json.dumps(event, ensure_ascii=False), flush=True)


def run(path, language, prompt, device, compute_type):
    model = WhisperModel(MODEL, device=device, compute_type=compute_type)
    segments, info = model.transcribe(
        path,
        language=language or None,
        initial_prompt=prompt or None,
        vad_filter=True,
        condition_on_previous_text=False,
        word_timestamps=True,
        beam_size=5,
    )
    emit(event="info", duration=info.duration, language=info.language, device=device)
    for s in segments:
        emit(
            event="segment",
            start=s.start,
            end=s.end,
            text=s.text.strip(),
            avg_logprob=s.avg_logprob,
            no_speech_prob=s.no_speech_prob,
            words=[{"start": w.start, "end": w.end, "word": w.word, "probability": w.probability} for w in (s.words or [])],
        )


def main():
    path = sys.argv[1]
    language = sys.argv[2] if len(sys.argv) > 2 else "es"
    prompt = sys.argv[3] if len(sys.argv) > 3 else ""
    try:
        run(path, language, prompt, "cuda", "int8_float16")
    except Exception as e:  # missing CUDA libraries, out of GPU memory, no GPU
        emit(event="log", message=f"GPU unavailable ({type(e).__name__}: {e}); using CPU")
        emit(event="reset")
        run(path, language, prompt, "cpu", "int8")


if __name__ == "__main__":
    main()
