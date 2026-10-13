/* The only code that touches LibRaw, with stream.cpp. Rust never mirrors
 * LibRaw's own structs, whose layout changes between releases; it sees only
 * drip_raw_info below, which this file fills by name against the headers it
 * is compiled with. */

#define _POSIX_C_SOURCE 200809L /* localtime_r under -std=c11 */

#include <libraw.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#define DRIP_CBLACK_SIZE 4104
#define DRIP_NOT_BAYER (-100000)

_Static_assert(LIBRAW_CBLACK_SIZE <= DRIP_CBLACK_SIZE, "cblack does not fit");

typedef struct {
  unsigned parsed_fields, illuminant;
  float calibration[4][4], color_matrix[4][3], forward_matrix[3][4];
} drip_dng_color;

typedef struct {
  int width, height; /* visible area */
  int cfa[2][2];     /* LibRaw color index (0 R, 1 G, 2 B, 3 G2) per row%2, col%2 */
  int flip;         /* LibRaw orientation bits; pixels remain in sensor order */
  unsigned black, maximum;
  unsigned cblack[DRIP_CBLACK_SIZE];
  float cam_xyz[4][3]; /* XYZ (D65) to camera */
  float cam_mul[4];    /* as-shot white balance multipliers */
  char make[64], model[64];
  float iso_speed, shutter, aperture, focal_len;
  long long timestamp;
  char datetime[20]; /* EXIF "YYYY:MM:DD HH:MM:SS", empty if unknown */
  unsigned raw_width;
  unsigned raw_height;
  unsigned left_margin;
  unsigned top_margin;
  double pixel_aspect;
  char artist[64];
  char description[512];
  unsigned shot_order;
  char body_serial[64];
  char internal_body_serial[64];
  char lens_make[128];
  char lens_model[128];
  char lens_serial[128];
  char lens_internal_serial[128];
  unsigned long long lens_id;
  char lens_maker_note_model[128];
  float lens_min_focal;
  float lens_max_focal;
  float lens_max_aperture_wide;
  float lens_max_aperture_tele;
  float focal_length_35mm;
  float min_focus_distance;
  unsigned lens_mount;
  unsigned camera_mount;
  unsigned lens_format;
  unsigned camera_format;
  char teleconverter[128];
  unsigned long long teleconverter_id;
  char adapter[128];
  unsigned long long adapter_id;
  char attachment[128];
  unsigned long long attachment_id;
  float latitude[3];
  float longitude[3];
  float gps_timestamp[3];
  float altitude;
  unsigned char latitude_ref;
  unsigned char longitude_ref;
  unsigned char altitude_ref;
  unsigned char gps_status;
  unsigned char gps_parsed;
  float pre_mul[4];
  float analog_balance[4];
  unsigned linear_max[4];
  float rgb_cam[3][4];
  float camera_matrix[3][4];
  unsigned dng_white_level[4];
  float dng_as_shot_neutral[4];
  float dng_user_crop[4];
  float baseline_exposure;
  unsigned inset_crops[2][4];
  int masked_areas[8][4];
  int shooting_modes[7];
  unsigned dng_default_crop[4];
  drip_dng_color dng_color[2];
} drip_raw_info;

/* LibRaw reads the zone-less EXIF capture time with mktime(), so localtime()
 * recovers the camera's wall-clock time (except within a repeated DST hour). */
static void exif_datetime(time_t timestamp, char out[20]) {
  struct tm t;
#ifdef _WIN32
  int ok = !localtime_s(&t, &timestamp);
#else
  int ok = localtime_r(&timestamp, &t) != NULL;
#endif
  if (!timestamp || !ok || !strftime(out, 20, "%Y:%m:%d %H:%M:%S", &t))
    out[0] = 0;
}

static int is_bayer(libraw_data_t *lr) {
  return lr->rawdata.raw_image && lr->idata.filters > 1000 && lr->idata.colors == 3 &&
         lr->sizes.top_margin + lr->sizes.height <= lr->sizes.raw_height &&
         lr->sizes.left_margin + lr->sizes.width <= lr->sizes.raw_width;
}

/* LibRaw reports every TIFF IFD and maker note entry it parses to ifd_entry
 * and note_entry with the context `entries`, during libraw_open_file only. */
int drip_raw_open(const char *path, exif_parser_callback ifd_entry,
                  exif_parser_callback note_entry, void *entries, void **handle,
                  drip_raw_info *info) {
  libraw_data_t *lr = libraw_init(0);
  if (!lr)
    return LIBRAW_UNSUFFICIENT_MEMORY;
  libraw_set_exifparser_handler(lr, ifd_entry, entries);
  libraw_set_makernotes_handler(lr, note_entry, entries);
  int err = libraw_open_file(lr, path);
  libraw_set_exifparser_handler(lr, NULL, NULL);
  libraw_set_makernotes_handler(lr, NULL, NULL);
  if (!err)
    err = libraw_unpack(lr);
  if (!err && !is_bayer(lr))
    err = DRIP_NOT_BAYER;
  if (err) {
    libraw_close(lr);
    return err;
  }
  info->width = lr->sizes.width;
  info->height = lr->sizes.height;
  info->flip = lr->sizes.flip;
  for (int r = 0; r < 2; r++)
    for (int c = 0; c < 2; c++)
      info->cfa[r][c] = libraw_COLOR(lr, r, c);
  info->black = lr->color.black;
  info->maximum = lr->color.maximum;
  memcpy(info->cblack, lr->color.cblack, sizeof lr->color.cblack);
  memcpy(info->cam_xyz, lr->color.cam_xyz, sizeof info->cam_xyz);
  memcpy(info->cam_mul, lr->color.cam_mul, sizeof info->cam_mul);
  memcpy(info->make, lr->idata.normalized_make, sizeof info->make);
  memcpy(info->model, lr->idata.normalized_model, sizeof info->model);
  info->make[sizeof info->make - 1] = info->model[sizeof info->model - 1] = 0;
  info->iso_speed = lr->other.iso_speed;
  info->shutter = lr->other.shutter;
  info->aperture = lr->other.aperture;
  info->focal_len = lr->other.focal_len;
  info->timestamp = lr->other.timestamp;
  exif_datetime(lr->other.timestamp, info->datetime);
  info->raw_width = lr->sizes.raw_width;
  info->raw_height = lr->sizes.raw_height;
  info->left_margin = lr->sizes.left_margin;
  info->top_margin = lr->sizes.top_margin;
  info->pixel_aspect = lr->sizes.pixel_aspect;
  memcpy(info->artist, lr->other.artist, sizeof info->artist);
  info->artist[sizeof info->artist - 1] = 0;
  memcpy(info->description, lr->other.desc, sizeof info->description);
  info->description[sizeof info->description - 1] = 0;
  info->shot_order = lr->other.shot_order;
  memcpy(info->body_serial, lr->shootinginfo.BodySerial, sizeof info->body_serial);
  info->body_serial[sizeof info->body_serial - 1] = 0;
  memcpy(info->internal_body_serial, lr->shootinginfo.InternalBodySerial, sizeof info->internal_body_serial);
  info->internal_body_serial[sizeof info->internal_body_serial - 1] = 0;
  memcpy(info->lens_make, lr->lens.LensMake, sizeof info->lens_make);
  info->lens_make[sizeof info->lens_make - 1] = 0;
  memcpy(info->lens_model, lr->lens.Lens, sizeof info->lens_model);
  info->lens_model[sizeof info->lens_model - 1] = 0;
  memcpy(info->lens_serial, lr->lens.LensSerial, sizeof info->lens_serial);
  info->lens_serial[sizeof info->lens_serial - 1] = 0;
  memcpy(info->lens_internal_serial, lr->lens.InternalLensSerial, sizeof info->lens_internal_serial);
  info->lens_internal_serial[sizeof info->lens_internal_serial - 1] = 0;
  info->lens_id = lr->lens.makernotes.LensID;
  memcpy(info->lens_maker_note_model, lr->lens.makernotes.Lens, sizeof info->lens_maker_note_model);
  info->lens_maker_note_model[sizeof info->lens_maker_note_model - 1] = 0;
  info->lens_min_focal = lr->lens.MinFocal;
  info->lens_max_focal = lr->lens.MaxFocal;
  info->lens_max_aperture_wide = lr->lens.MaxAp4MinFocal;
  info->lens_max_aperture_tele = lr->lens.MaxAp4MaxFocal;
  info->focal_length_35mm = lr->lens.FocalLengthIn35mmFormat ? lr->lens.FocalLengthIn35mmFormat : lr->lens.makernotes.FocalLengthIn35mmFormat;
  info->min_focus_distance = lr->lens.makernotes.MinFocusDistance;
  info->lens_mount = lr->lens.makernotes.LensMount;
  info->camera_mount = lr->lens.makernotes.CameraMount;
  info->lens_format = lr->lens.makernotes.LensFormat;
  info->camera_format = lr->lens.makernotes.CameraFormat;
  memcpy(info->teleconverter, lr->lens.makernotes.Teleconverter, sizeof info->teleconverter);
  info->teleconverter[sizeof info->teleconverter - 1] = 0;
  info->teleconverter_id = lr->lens.makernotes.TeleconverterID;
  memcpy(info->adapter, lr->lens.makernotes.Adapter, sizeof info->adapter);
  info->adapter[sizeof info->adapter - 1] = 0;
  info->adapter_id = lr->lens.makernotes.AdapterID;
  memcpy(info->attachment, lr->lens.makernotes.Attachment, sizeof info->attachment);
  info->attachment[sizeof info->attachment - 1] = 0;
  info->attachment_id = lr->lens.makernotes.AttachmentID;
  memcpy(info->latitude, lr->other.parsed_gps.latitude, sizeof info->latitude);
  memcpy(info->longitude, lr->other.parsed_gps.longitude, sizeof info->longitude);
  memcpy(info->gps_timestamp, lr->other.parsed_gps.gpstimestamp, sizeof info->gps_timestamp);
  info->altitude = lr->other.parsed_gps.altitude;
  info->latitude_ref = lr->other.parsed_gps.latref;
  info->longitude_ref = lr->other.parsed_gps.longref;
  info->altitude_ref = lr->other.parsed_gps.altref;
  info->gps_status = lr->other.parsed_gps.gpsstatus;
  info->gps_parsed = lr->other.parsed_gps.gpsparsed;
  memcpy(info->pre_mul, lr->color.pre_mul, sizeof info->pre_mul);
  memcpy(info->analog_balance, lr->other.analogbalance, sizeof info->analog_balance);
  memcpy(info->linear_max, lr->color.linear_max, sizeof info->linear_max);
  memcpy(info->rgb_cam, lr->color.rgb_cam, sizeof info->rgb_cam);
  memcpy(info->camera_matrix, lr->color.cmatrix, sizeof info->camera_matrix);
  memcpy(info->dng_white_level, lr->color.dng_levels.dng_whitelevel, sizeof info->dng_white_level);
  memcpy(info->dng_as_shot_neutral, lr->color.dng_levels.asshotneutral, sizeof info->dng_as_shot_neutral);
  memcpy(info->dng_user_crop, lr->color.dng_levels.user_crop, sizeof info->dng_user_crop);
  info->baseline_exposure = lr->color.dng_levels.baseline_exposure;
  for (int i = 0; i < 2; i++) {
    info->inset_crops[i][0] = lr->sizes.raw_inset_crops[i].cleft;
    info->inset_crops[i][1] = lr->sizes.raw_inset_crops[i].ctop;
    info->inset_crops[i][2] = lr->sizes.raw_inset_crops[i].cwidth;
    info->inset_crops[i][3] = lr->sizes.raw_inset_crops[i].cheight;
    info->dng_color[i].parsed_fields = lr->color.dng_color[i].parsedfields;
    info->dng_color[i].illuminant = lr->color.dng_color[i].illuminant;
    memcpy(info->dng_color[i].calibration, lr->color.dng_color[i].calibration, sizeof info->dng_color[i].calibration);
    memcpy(info->dng_color[i].color_matrix, lr->color.dng_color[i].colormatrix, sizeof info->dng_color[i].color_matrix);
    memcpy(info->dng_color[i].forward_matrix, lr->color.dng_color[i].forwardmatrix, sizeof info->dng_color[i].forward_matrix);
  }
  memcpy(info->masked_areas, lr->sizes.mask, sizeof info->masked_areas);
  for (int i = 0; i < 4; i++) info->dng_default_crop[i] = lr->color.dng_levels.default_crop[i];
  info->shooting_modes[0] = lr->shootinginfo.DriveMode;
  info->shooting_modes[1] = lr->shootinginfo.FocusMode;
  info->shooting_modes[2] = lr->shootinginfo.MeteringMode;
  info->shooting_modes[3] = lr->shootinginfo.AFPoint;
  info->shooting_modes[4] = lr->shootinginfo.ExposureMode;
  info->shooting_modes[5] = lr->shootinginfo.ExposureProgram;
  info->shooting_modes[6] = lr->shootinginfo.ImageStabilization;
  *handle = lr;
  return 0;
}

/* Copies the visible area into dst, which holds width * height values. */
void drip_raw_copy(void *handle, unsigned short *dst) {
  libraw_data_t *lr = handle;
  const libraw_image_sizes_t *s = &lr->sizes;
  for (int row = 0; row < s->height; row++)
    memcpy(dst + (size_t)row * s->width,
           lr->rawdata.raw_image + (size_t)(row + s->top_margin) * (s->raw_pitch / 2) + s->left_margin,
           (size_t)s->width * sizeof *dst);
}

void drip_raw_close(void *handle) { libraw_close(handle); }

const char *drip_raw_strerror(int err) {
  return err == DRIP_NOT_BAYER ? "not a 3-color Bayer raw" : libraw_strerror(err);
}

int drip_raw_unsupported(int err) {
  return err == LIBRAW_FILE_UNSUPPORTED || err == LIBRAW_NOT_IMPLEMENTED;
}

#ifdef DRIP_REFERENCE
/* LibRaw's own pipeline in half-size mode (2x2 binning, greens averaged), as-shot
 * white balance, no highlight recovery, linear Rec.2020, 16 bit, unrotated.
 * On success *rgb is a malloc'd width * height * 3 buffer the caller frees. */
int drip_raw_reference(const char *path, int *width, int *height, unsigned short **rgb) {
  libraw_data_t *lr = libraw_init(0);
  if (!lr)
    return LIBRAW_UNSUFFICIENT_MEMORY;
  lr->params.half_size = 1;
  lr->params.use_camera_wb = 1;
  lr->params.output_color = 8; /* Rec. 2020 */
  lr->params.gamm[0] = lr->params.gamm[1] = 1;
  lr->params.no_auto_bright = 1;
  lr->params.highlight = 0;
  lr->params.output_bps = 16;
  lr->params.user_flip = 0;
  int err = libraw_open_file(lr, path);
  if (!err)
    err = libraw_unpack(lr);
  if (!err)
    err = libraw_dcraw_process(lr);
  libraw_processed_image_t *img = err ? NULL : libraw_dcraw_make_mem_image(lr, &err);
  if (img && (img->type != LIBRAW_IMAGE_BITMAP || img->colors != 3 || img->bits != 16 ||
              img->data_size != (unsigned)img->width * img->height * 3 * 2)) {
    libraw_dcraw_clear_mem(img);
    img = NULL;
    err = DRIP_NOT_BAYER;
  }
  if (img) {
    *width = img->width;
    *height = img->height;
    *rgb = malloc(img->data_size);
    if (*rgb)
      memcpy(*rgb, img->data, img->data_size);
    else
      err = LIBRAW_UNSUFFICIENT_MEMORY;
    libraw_dcraw_clear_mem(img);
  }
  libraw_close(lr);
  return err;
}

void drip_free(void *p) { free(p); }
#endif
