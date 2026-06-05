# basic_text fixture

이 fixture는 변환 정확도 검증의 첫 기준점이다.

아직 `input.hwp`와 `input.hwpx`는 추가하지 않았다. 두 파일은 가능하면 같은 의미의 문서여야 하며, 아래 내용을 포함한다.

- `기본 한글 문단`
- `English 123 mixed text`
- `줄바꿈 앞`과 `줄바꿈 뒤` 사이에 문단 내부 줄바꿈 1개
- `탭 앞`과 `탭 뒤` 사이에 탭 문자 1개
- 빈 문단 1개

현재 기대 동작:

- `bridge::rhwp::read_document`가 HWP/HWPX 모두에서 성공한다.
- 비어 있지 않은 문단 4개만 `Block::Paragraph`로 남는다.
- 줄바꿈은 `Inline::LineBreak`, 탭은 `Inline::Tab`으로 보존된다.
- `txt`, `json`, `markdown`, `html`, `svg` export가 모두 성공한다.
