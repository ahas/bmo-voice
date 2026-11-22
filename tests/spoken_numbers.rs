//! The check's reading of numbers (docs/boundary-api.md, 「The spoken words」): spoken words that say every number
//! of the written words in a reading its language gives it pass, and those that say one in none are refused, naming
//! the number and how the rules read it.

use bmo_voice::script::numbers::misread;
use bmo_voice::script::reading::Language;
use bmo_voice::script::unconverted::{Finding, found};

/// Written words and spoken words that say every number of them as written, each in a reading its context allows.
const SAID: [(&str, &str); 24] = [
  (
    "검증 3893번, CPU 12% 사용 중입니다.",
    "검증 삼천팔백 구십삼번, 시피유 십이퍼센트 사용 중입니다.",
  ),
  ("3번째 시도에서 12개 파일을 고쳤습니다.", "세번째 시도에서 열두개 파일을 고쳤습니다."),
  ("1번째 줄", "첫번째 줄"),
  ("41명과 20살", "마흔한명과 스무살"),
  ("1, 2, 3", "하나, 둘, 셋"),
  (
    "응답 시간은 1.2초, 오류율은 0.5%입니다.",
    "응답 시간은 일쩌미초, 오류율은 영쩌모퍼센트입니다.",
  ),
  ("사용률은 31.5%입니다.", "사용률은 삼십일 점 오 퍼센트입니다."),
  ("99.5점", "구십구쩌모점"),
  ("처리량은 13,615건입니다.", "처리량은 만삼천육백 십오건입니다."),
  ("2026년 11월 23일 오후 3시", "이천 이십육년 십일월 이십삼일 오후 세 시"),
  ("6월과 10월", "유월과 시월"),
  ("12개월 동안 22시에", "십이개월 동안 오후 열시에"),
  ("14:30에 시작", "오후 두시 삼십분에 시작"),
  ("접속 코드는 0427입니다.", "접속 코드는 공사이칠입니다."),
  ("1만 명", "만 명"),
  ("50% 할인", "오십프로 할인"),
  ("버전 0.1.0", "버전 영쩜일쩜영"),
  ("2026.11.23 배포", "이천이십육년 십일월 이십삼일 배포"),
  ("RTX4090와 S2-Pro", "알티엑스사공구공과 에스투프로"),
  ("USB 3.2와 Wi-Fi 6을 지원합니다.", "유에스비쓰리쩜투와 와이파이식스를 지원합니다."),
  ("커밋 ebfee93d", "커밋 이비에프이"),
  ("`sleep 5`를 실행", "잠깐 멈추는 명령을 실행"),
  (
    "CPU at 12%, 1,250 rows in 2026.",
    "CPU at twelve percent, one thousand two hundred and fifty rows in twenty twenty-six.",
  ),
  ("It took 3.5 seconds at 14:05.", "It took three point five seconds at two oh five."),
];

/// Spoken words that say a number otherwise than as written, the number the check names, and the language.
const MISREAD: [(&str, &str, &str, Language); 12] = [
  ("CPU 12% 사용 중", "시피유 시비퍼센트 사용 중", "12%", Language::Ko),
  ("12개 파일", "시비개 파일", "12", Language::Ko),
  ("11월 23일", "시비뤌 이십삼일", "11", Language::Ko),
  ("CPU 12%", "시피유 십이", "12%", Language::Ko),
  ("99.5점", "구십꾸쩌모점", "99.5", Language::Ko),
  ("3893번", "삼천팔백 구십번", "3893", Language::Ko),
  ("12개 중 3개", "열두개 중 네개", "3", Language::Ko),
  ("10시", "시시", "10", Language::Ko),
  ("12개 파일", "십이개 파일", "12", Language::Ko),
  ("3시에", "삼시에", "3", Language::Ko),
  ("6월", "육월", "6", Language::Ko),
  ("CPU at 12%", "CPU at twenty percent", "12%", Language::En),
];

#[test]
fn spoken_words_that_say_each_number_in_a_reading_it_has_pass() {
  for (written, spoken) in SAID {
    let language = Language::of_line(written).unwrap_or(Language::Ko);

    assert_eq!(found(written, spoken, language), None, "{written} said as {spoken}");
  }
}

#[test]
fn a_number_said_in_no_reading_it_has_is_refused_naming_it_and_its_reading() {
  for (written, spoken, number, language) in MISREAD {
    let misread =
      misread(written, spoken, language).unwrap_or_else(|| panic!("{spoken} passed for {written}"));

    assert_eq!(misread.written, number, "{written} said as {spoken}");
  }

  let refused = found("CPU 12% 사용 중", "시피유 시비퍼센트 사용 중", Language::Ko).expect("12 misread");

  assert!(matches!(refused, Finding::Misread(_)), "{refused:?}");
  assert!(refused.to_string().contains("\"12%\""), "{refused}");
  assert!(refused.to_string().contains("십이 퍼센트"), "the rules' reading is named: {refused}");

  let english = misread("CPU at 12%", "CPU at twenty percent", Language::En).expect("12 misread");

  assert_eq!(english.said, "twelve percent");
}

#[test]
fn a_word_left_unconverted_is_found_before_a_number() {
  let refused = found("CPU 12%", "CPU 시비퍼센트", Language::Ko).expect("a Latin word");

  assert!(matches!(refused, Finding::Unconverted(_)), "{refused:?}");
}
