import QRCode from "qrcode";
import { useMemo } from "react";

/**
 * The pairing QR, drawn as real SVG elements.
 *
 * This used to be `dangerouslySetInnerHTML` fed by `QRCode.toString()`. The
 * markup came from a local library and carried no remote input, so it was not
 * exploitable — but this file is about to be public, and the first thing a
 * reviewer greps for in a repo that holds exchange credentials is exactly that
 * attribute. A safety argument that has to be reconstructed from a comment is
 * weaker than one the code cannot violate.
 *
 * `QRCode.create` hands back the module matrix synchronously, so the drawing
 * is ours: no HTML string is ever produced, and there is no async gap in which
 * a stale code could be shown for a key that has already been rotated.
 */

export interface QrMatrix {
  /** Modules per side, excluding the quiet zone. */
  size: number;
  /** SVG path covering every dark module. */
  path: string;
}

/**
 * Turns the module matrix into one path, merging horizontal runs.
 *
 * Pure and exported so the geometry can be checked without a DOM: a QR that
 * renders but encodes the wrong thing looks identical to a correct one.
 */
export function qrMatrix(text: string): QrMatrix | null {
  if (!text) return null;
  let modules;
  try {
    modules = QRCode.create(text, { errorCorrectionLevel: "M" }).modules;
  } catch {
    // Failing to encode must not draw a partial code. A QR that looks scannable
    // and is not costs the user a pairing attempt and teaches them to doubt the
    // countdown.
    return null;
  }
  const size = modules.size;
  const data = modules.data;
  const parts: string[] = [];
  for (let y = 0; y < size; y += 1) {
    let x = 0;
    while (x < size) {
      if (!data[y * size + x]) {
        x += 1;
        continue;
      }
      const start = x;
      while (x < size && data[y * size + x]) x += 1;
      parts.push(`M${start} ${y}h${x - start}v1h-${x - start}z`);
    }
  }
  return { size, path: parts.join("") };
}

/** One quiet-zone module on each side — below this, scanners lose the finder. */
const QUIET = 1;

export function QrCode({ text, label }: { text: string; label: string }) {
  const matrix = useMemo(() => qrMatrix(text), [text]);
  if (!matrix) return null;
  const span = matrix.size + QUIET * 2;
  return (
    <svg
      viewBox={`0 0 ${span} ${span}`}
      role="img"
      aria-label={label}
      shapeRendering="crispEdges"
    >
      {/* The light field is drawn, not inherited. A QR on a dark theme with a
          transparent background is unscannable, and it fails silently. */}
      <rect width={span} height={span} fill="#ffffff" />
      <g transform={`translate(${QUIET} ${QUIET})`}>
        <path d={matrix.path} fill="#000000" />
      </g>
    </svg>
  );
}
