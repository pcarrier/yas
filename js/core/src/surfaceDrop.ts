/** Names shared by the browser's hover offer and its final file payload. */
export function plannedDropExtension(mime: string): string | null {
  switch (mime.split(";", 1)[0].trim().toLowerCase()) {
    case "image/png":
      return "png";
    case "image/jpeg":
      return "jpg";
    case "image/webp":
      return "webp";
    case "image/gif":
      return "gif";
    case "image/avif":
      return "avif";
    case "image/heic":
      return "heic";
    case "image/heif":
      return "heif";
    case "image/tiff":
      return "tiff";
    case "image/bmp":
      return "bmp";
    default:
      return null;
  }
}

export function plannedDropName(mime: string, index: number): string {
  return `${index}.${plannedDropExtension(mime) ?? "bin"}`;
}
