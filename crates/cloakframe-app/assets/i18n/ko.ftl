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
state-cancelling = 취소하는 중
summary-saved = { $count }개 저장
summary-review = { $count }개 검토 필요
summary-failed = { $count }개 실패
summary-cancelled = { $count }개 취소
summary-none = 저장한 파일 없음
summary-with-details = { $counts } — { $details }
summary-missed-faces = 영상 { $videos }개에서 얼굴을 { $seconds }초간 놓침
summary-missed-plates = 영상 { $videos }개에서 번호판을 { $seconds }초간 놓침
summary-capped = { $count }개 항목에서 감지 개수 상한에 닿음
summary-weak = { $count }개 항목에 확실하지 않은 감지
summary-failed-frames = 영상 { $count }개에 분석하지 못한 프레임
list-separator = {", "}
problem-missed-face = { $first }–{ $last } 프레임에서 얼굴을 놓침({ $seconds }초)
problem-missed-plate = { $first }–{ $last } 프레임에서 번호판을 놓침({ $seconds }초)
problem-capped = 감지 개수 상한에 닿음: { $found }개 중 { $kept }개만 남김
problem-failed-frame = { $frame } 프레임을 분석하지 못함
problem-weak = 확실하지 않은 감지(점수 { $score })
progress-percent = { $percent }%
progress-left-estimating = 남은 시간 계산 중
progress-left-under-minute = 1분 안에 끝남
progress-left-minutes = 약 { $minutes }분 남음
progress-left-hours = 약 { $hours }시간 { $minutes }분 남음
run-session-kept = 검토할 항목을 저장했습니다. 앱을 다시 열어도 목록에 남습니다.
item-reveal = 폴더에서 보기
item-open = 열기
item-reveal-source = 원본 보기
shell-failed = { $path }을(를) 보여 주지 못했습니다: { $error }
log-title = 자세히
log-copy = 복사
quit-title = 작업이 진행 중입니다
quit-body = 끝내면 작업을 취소합니다. 저장한 파일은 그대로이고, 검토를 기다리는 항목은 나중을 위해 남깁니다.
quit-confirm = 취소하고 끝내기
quit-keep = 계속하기
quit-waiting = 멈추는 중입니다. 작업이 멈추면 창이 닫힙니다.

sessions-title = 검토를 기다리는 작업
session-age = { $days ->
    [0] 오늘
    [1] 어제
   *[other] { $days }일 전
}
session-row = { $output }에 저장할 항목 { $items }개
session-changed = 원본이 바뀐 항목 { $count }개
session-unreadable = 읽을 수 없습니다: { $error }
session-open = 열기
session-discard = 버리기…
session-back = 돌아가기
session-item-changed = 원본이 바뀌었거나 없습니다. 다시 실행하세요
discard-title = 이 검토를 버릴까요?
discard-body = 검토를 위해 남긴 감지 결과를 지웁니다. 원본과 저장한 파일은 그대로입니다.
discard-confirm = 버리기
discard-keep = 그대로 두기
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
