# merged_table fixture

이 fixture는 병합 셀 정보가 실제 HWP 입력에서 `Document IR`의 `TableCell.row_span`과 `TableCell.col_span`으로 보존되는지 검증한다.

현재 `input.hwp`가 준비되어 있다. `input.hwpx`는 아직 추가하지 않았다.

문서 내용:

- 3행 3열 표 1개
- `row span` cell은 세로로 2행을 차지한다.
- `col span` cell은 가로로 2열을 차지한다.
- 나머지 owner cell은 `cell 2-2`, `cell 2-3`, `cell 3-1`, `cell 3-2`, `cell 3-3` 텍스트를 가진다.

현재 기대 동작:

- `bridge::rhwp::read_document`가 준비된 입력 파일에서 성공한다.
- 본문 block으로 `Block::Table` 1개가 남는다.
- 표는 row 3개를 가진다.
- 최소 하나의 cell이 `row_span = 2`를 가진다.
- 최소 하나의 cell이 `col_span = 2`를 가진다.
- owner cell 텍스트가 row-major 순서로 보존된다.
- `txt`, `json`, `markdown`, `html`, `svg` export가 모두 성공한다.

현재 한계:

- 이 fixture는 병합 정보가 IR에 남는지를 본다.
- 병합 셀의 시각적 너비/높이와 실제 렌더링 위치까지 검증하지 않는다.

`expected/bridge-stats.hwp.json`은 현재 HWP 관찰값을 회귀 기준으로 고정한다.
