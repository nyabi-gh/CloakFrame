# The FFmpeg components the bundled build enables, sourced by build.sh. Only what the engine
# asks of FFmpeg (crates/cloakframe-engine/src/media) and what the ffmpeg command inserts on its
# own (fftools/ffmpeg_filter.c: autorotation, container cropping, trimming, format conversion)
# is built, so a crafted file meets as little parsing code as possible (docs/M7_FFMPEG.md).
#
# Inputs are MP4/MOV/M4V and WebM (plan 1.1). The audio decoders cover what those containers
# carry, since audio that cannot be copied into the MP4 is decoded and encoded again as AAC.
# The `null` muxer encodes with wrapped_avframe and pcm_s16le when the output check
# (media/verify.rs) decodes a whole output.

ffmpeg_audio_decoders=(
    aac aac_latm mp3float mp2float ac3 eac3 alac opus vorbis flac dca truehd mlp amrnb amrwb
    adpcm_ima_qt pcm_s8 pcm_u8 pcm_s16le pcm_s16be pcm_s24le pcm_s24be pcm_s32le pcm_s32be
    pcm_f32le pcm_f32be pcm_f64le pcm_f64be pcm_alaw pcm_mulaw
)

ffmpeg_list() {
    local IFS=,
    echo "$*"
}

ffmpeg_components=(
    --enable-protocol=file,pipe
    --enable-demuxer=mov,matroska,rawvideo
    --enable-muxer=mp4,null,rawvideo
    --enable-decoder="h264,hevc,vp8,vp9,rawvideo,$(ffmpeg_list "${ffmpeg_audio_decoders[@]}")"
    --enable-encoder=libx264,libx265,aac,rawvideo,wrapped_avframe,pcm_s16le
    --enable-parser=h264,hevc,vp8,vp9,aac,aac_latm,mpegaudio,ac3,opus,vorbis,flac,dca,mlp,amr
    --enable-bsf=setts,noise,aac_adtstoasc,null
    --enable-filter=setsar,scale,format,aformat,aresample,select,setparams,trim,atrim,null,anull,crop,transpose,hflip,vflip,rotate
)
