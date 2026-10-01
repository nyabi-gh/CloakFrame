# Models and third-party notices

This page records the origin and practical license constraints of CloakFrame's
detection models. It is a summary, not a substitute for the upstream license
text. Confirm upstream terms before commercial use or redistribution.

## Built-in models

Built-in models are not committed to this repository or bundled with release
packages. CloakFrame downloads each model from its pinned upstream location on
first use, verifies its SHA-256 digest, and caches it locally.

### YOLO5Face-n

The recommended face detector is an ONNX conversion from the
[yolov5-face-onnx-inference release](https://github.com/yakhyo/yolov5-face-onnx-inference/releases/tag/weights).
It is based on [YOLO5Face](https://github.com/deepcam-cn/yolov5-face), whose
code is GPL-3.0, and was trained on WIDER FACE. WIDER FACE is restricted to
non-commercial research use, so treat this model as **non-commercial only**
under terms separate from the CloakFrame application license.

### YuNet

The fast face detector is `face_detection_yunet_2023mar.onnx` from the
[OpenCV Zoo YuNet directory](https://github.com/opencv/opencv_zoo/tree/main/models/face_detection_yunet).
That directory identifies the model license as MIT.

### License-plate detector

The built-in plate detector comes from
[open-image-models](https://github.com/ankandrew/open-image-models), which is
MIT-licensed. It uses the YOLOv9 architecture and remains subject to its
upstream project's terms.

### Custom SCRFD models

Custom SCRFD files can be selected with **Browse…**. Their terms depend on the
provider. InsightFace pretrained models are limited to non-commercial research
use; mirrors and file conversions do not replace those upstream terms. Load
only models from sources you trust.

## Application and runtime dependencies

CloakFrame source is licensed under GNU GPL v3.0 or later. Earlier releases
before v1.1.0 remain under their original PolyForm Noncommercial 1.0.0 terms.

Runtime dependencies retain their own licenses, including Qt (LGPL-3.0,
GPL-3.0, or commercial), OpenCV (Apache-2.0), ONNX Runtime (MIT), DirectML
(Microsoft redistribution terms), Exiv2 (GPL-2.0-or-later), spdlog and fmt
(MIT), Velopack and Sparkle (MIT), and FFmpeg (the bundled builds are
GPL-3.0-or-later; the FFmpeg core is LGPL-2.1-or-later). Complete notices and bundled license texts are in
[THIRD_PARTY_NOTICES.txt](../THIRD_PARTY_NOTICES.txt).

## Citations

```bibtex
@misc{guo2021sample,
  title={Sample and Computation Redistribution for Efficient Face Detection},
  author={Jia Guo and Jiankang Deng and Alexandros Lattas and Stefanos Zafeiriou},
  year={2021},
  eprint={2105.04714},
  archivePrefix={arXiv},
  primaryClass={cs.CV}
}

@misc{qi2021yolo5face,
  title={YOLO5Face: Why Reinventing a Face Detector},
  author={Delong Qi and Weijun Tan and Qi Yao and Jingfeng Liu},
  year={2021},
  eprint={2105.12931},
  archivePrefix={arXiv},
  primaryClass={cs.CV}
}

@misc{wang2024yolov9,
  title={YOLOv9: Learning What You Want to Learn Using Programmable Gradient Information},
  author={Chien-Yao Wang and Hong-Yuan Mark Liao},
  year={2024},
  eprint={2402.13616},
  archivePrefix={arXiv},
  primaryClass={cs.CV}
}
```
