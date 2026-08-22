/** Browser codec capabilities used when selecting native Surface formats. */
export const CODEC_SUPPORT_H264 = 1 << 0;
export const CODEC_SUPPORT_AV1 = 1 << 1;
export const CODEC_SUPPORT_H264_444 = 1 << 2;
export const CODEC_SUPPORT_AV1_444 = 1 << 3;

/** Private browser decoder flags derived from validated native Surface state. */
export const SURFACE_FRAME_FLAG_KEYFRAME = 1 << 0;
export const SURFACE_FRAME_CODEC_MASK = 0b110;
export const SURFACE_FRAME_CODEC_H264 = 0 << 1;
export const SURFACE_FRAME_CODEC_AV1 = 1 << 1;
export const SURFACE_FRAME_CODEC_PNG = 2 << 1;
