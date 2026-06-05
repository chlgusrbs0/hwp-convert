# style fixture

이 fixture는 실제 HWP 입력에서 글자 스타일과 문단 스타일이 `Document IR`로 보존되는지 검증한다.

현재 `input.hwp`가 준비되어 있다. `input.hwpx`는 아직 추가하지 않았다.

문서 내용:

- 텍스트 `styled text`를 가진 문단 1개
- 글자 스타일: bold, italic, underline, strike
- 글꼴: `Noto Sans KR`
- 글자 크기: 12pt
- 글자색: RGB `03, 02, 01`
- 배경색: RGB `06, 05, 04`
- 문단 정렬: center
- 문단 spacing before 4pt, after 5pt
- 문단 left indent 3pt

현재 기대 동작:

- `bridge::rhwp::read_document`가 준비된 입력 파일에서 성공한다.
- `styled text` 문단이 남는다.
- 위 글자/문단 스타일이 `TextStyle`과 `ParagraphStyle`에 보존된다.
- `txt`, `json`, `markdown`, `html`, `svg` export가 모두 성공한다.

현재 한계:

- 이 fixture는 style 정보가 semantic IR에 남는지를 본다.
- 모든 exporter가 모든 style을 표현하는지까지 동일하게 요구하지 않는다.

`expected/bridge-stats.hwp.json`은 현재 HWP 관찰값을 회귀 기준으로 고정한다.
