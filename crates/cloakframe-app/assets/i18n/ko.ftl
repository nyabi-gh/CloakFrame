app-name = CloakFrame

drop-hint = 이미지, 동영상, 폴더를 여기로 드래그하세요
drop-release = 놓으면 { $count }개 추가
add-files = 파일 추가…
add-folder = 폴더 추가…
clear-list = 비우기
scanning = 사진과 동영상을 찾는 중…
found-summary = 사진 { $photos }개, 동영상 { $videos }개
skipped-type = { $extension } 형식 파일 { $count }개는 처리할 수 없습니다
skipped-no-extension = 확장자가 없는 파일 { $count }개는 처리할 수 없습니다
unreadable = { $path }을(를) 읽을 수 없습니다

what = 가릴 대상
faces = 얼굴
plates = 번호판
how = 가리는 방법
method-fill = 채우기
method-mosaic = 모자이크
method-blur = 블러
strength = 강도
models-need-one = 얼굴이나 번호판 중 하나 이상을 고르세요.

output-folder = 저장 위치
choose-folder = 선택…
no-output-folder = 폴더를 고르지 않았습니다
start = 시작
start-needs-files = 먼저 사진이나 동영상을 추가하세요.
start-needs-output = 먼저 저장 위치를 고르세요.

menu-app-about = CloakFrame 정보
menu-app-quit = CloakFrame 종료
menu-file = 파일
menu-file-quit = 끝내기
menu-edit = 편집
menu-edit-undo = 실행 취소
menu-edit-redo = 다시 실행
menu-edit-cut = 오려두기
menu-edit-copy = 복사
menu-edit-paste = 붙여넣기
menu-edit-select-all = 전체 선택
menu-view = 보기
menu-help = 도움말

theme = 테마
theme-system = 시스템
theme-light = 라이트
theme-dark = 다크
language = 언어
language-system = 시스템

about-title = CloakFrame 정보
about-version = 버전 { $version }
about-license = CloakFrame은 Apache License 2.0을 따르는 자유 소프트웨어입니다.
about-third-party = 서드파티 고지
about-models = 감지 모델
close = 닫기

settings-unknown-keys = 이 버전이 모르는 설정은 그대로 두었습니다: { $keys }
settings-unreadable = 설정 파일을 읽을 수 없어 그대로 두었습니다: { $error }
settings-newer = 새 버전의 CloakFrame이 쓴 설정입니다(형식 { $version }). 여기서 바꾼 것은 저장하지 않습니다.

review-first = 저장하기 전에 검토
running = 처리하는 중…
cancel = 취소
state-queued = 대기
state-analyzed = 분석함
state-needs-review = 검토 필요
state-rendering = 저장 중
state-verified = 확인함
state-published = 저장함
state-failed = 실패
state-cancelled = 취소됨
run-summary = 저장 { $saved }개, 검토 대기 { $review }개, 실패 { $failed }개
run-new = 새로 시작
start-needs-runtime = 앱 옆에서 ONNX Runtime을 찾지 못했습니다.
model-missing = 감지 모델 { $name }을(를) 아직 받지 않았습니다.
videos-need-ffmpeg = 앱 옆에서 FFmpeg를 찾지 못해 동영상은 뺐습니다.

models-title = 감지 모델
models-open = 감지 모델…
models-folder = 폴더: { $path }
model-name-yolo5face-n = YOLO5Face-n (얼굴, 추천)
model-name-yunet = YuNet (얼굴, 빠름)
model-name-plates = YOLOv9-t (번호판)
model-terms-yolo5face-n = YOLO5Face(GPL-3.0)에서 변환했고 비상업 연구용 데이터셋 WIDER FACE로 학습했습니다. 비상업 목적으로만 쓰세요.
model-terms-yunet = OpenCV Zoo에서 MIT 라이선스로 받습니다.
model-terms-plates = open-image-models 프로젝트에서 MIT 라이선스로 받습니다. 바탕이 된 YOLOv9의 조건도 적용됩니다.
model-size = { $size } MB
model-license = 라이선스: { $license }
model-present = 받음
model-not-downloaded = 받지 않음
model-different = 그 자리의 파일은 이 모델이 아닙니다. 받으면 바꿉니다.
model-accept-terms = 이 조건에 동의합니다
model-download = 받기
model-download-failed = 받지 못했습니다: { $error }
