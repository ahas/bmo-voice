//! The check's reading of the words a line's spoken words keep as written (docs/boundary-api.md, 「The spoken
//! words」): spoken words that say every Hangul word of a Korean line in its place pass, whatever the rules change at
//! its joins to the figures, names and code around it, and those that leave one out or move it are refused, naming
//! the word, and asked of the model again naming it.

use bmo_voice::script::reading::Language;
use bmo_voice::script::unconverted::{Finding, found, found_in_reading};
use bmo_voice::speech::spoken::testing;

/// Written words and spoken words that keep every Hangul word, each changed only as the rules allow.
const KEPT: [(&str, &str); 11] = [
  // The breaks a speaker does not make are closed up.
  (
    "응답 시간은 1.2초, 오류율은 0.5%입니다.",
    "응답시간은 일쩌미초, 오류율은 영쩌모퍼센트입니다.",
  ),
  // A particle written onto a figure or a name is fitted to how it is said.
  ("RTX4090와 Wi-Fi 6을 지원합니다.", "알티엑스사공구공과 와이파이식스를 지원합니다."),
  // A name's final consonant is carried into a particle that opens on no sound.
  ("SQL을 씁니다.", "에스큐에를 씁니다."),
  // A run's own final consonant is carried onto the name after it.
  ("오픈AI가 발표했습니다.", "오프네이아이가 발표했습니다."),
  // The copula written onto a name contracts or not as its reading ends.
  ("사진은 JPEG였고 크기는 작았어요.", "사진은 제이펙이었고 크기는 작았어요."),
  // A path, an address and code are said in a few words or left out.
  ("graphql()과 두 함수가 있습니다.", "두 함수가 있습니다."),
  ("docs/설정.md를 고쳤습니다.", "설정 문서를 고쳤습니다."),
  ("`설정 파일`을 읽고 https://예시.com/경로에서 받습니다.", "설정 파일을 읽고 받습니다."),
  ("\u{E001}임시 변수\u{E002} 값을 지웁니다.", "값을 지웁니다."),
  // Words a time is told in are added.
  ("15:30에 시작합니다.", "오후 세시 삼십분에 시작합니다."),
  // An English line's words are not looked for.
  ("The 오류율 is 0.5%.", "The error rate is zero point five percent."),
];

#[test]
fn spoken_words_that_keep_every_hangul_word_in_its_place_pass() {
  for (written, spoken) in KEPT {
    let language = if written.starts_with("The") { Language::En } else { Language::Ko };

    assert_eq!(found(written, spoken, language), None, "{written} as {spoken}");
  }
}

#[test]
fn a_hangul_word_left_out_moved_or_respelled_is_refused_naming_it() {
  let written = "3번째 시도에서 12개 파일을 고쳤고 응답 시간은 1.2초, 오류율은 0.5%입니다.";
  let refused = [
    // The live line: the rate's word left out.
    (
      "세번째 시도에서 열두개 파일을 고쳤고 응답 시간은 일쩌미초, 영쩌모퍼센트입니다.",
      "오류율은",
    ),
    // Two words swapped.
    (
      "세번째 시도에서 열두개 고쳤고 파일을 응답 시간은 일쩌미초, 오류율은 영쩌모퍼센트입니다.",
      "고쳤고",
    ),
    // An ordinary word written as it sounds.
    (
      "세번째 시도에서 열두개 파이를 고쳤고 응답 시간은 일쩌미초, 오류율은 영쩌모퍼센트입니다.",
      "파일을",
    ),
    // A word after a number left out.
    (
      "세번째 시도에서 열두개 파일을 고쳤고 응답 시간은 일쩌미초, 오류율은 영쩌모퍼센트.",
      "0.5%입니다",
    ),
  ];

  for (spoken, word) in refused {
    let finding = found(written, spoken, Language::Ko);

    assert!(
      matches!(&finding, Some(Finding::Dropped(dropped)) if dropped.word == word),
      "{spoken}: {finding:?}"
    );
    assert!(finding.unwrap().to_string().contains(&format!("\"{word}\"")));
  }
}

#[test]
fn a_hangul_spelling_the_agreed_readings_list_stands_as_its_reading() {
  let written = "에이하스는 작업을 끝냈습니다.";
  let answer = r#"{"spoken": "아하스는 작업을 끝냈습니다.", "readings": []}"#;
  let agreed = [("에이하스".to_owned(), "아하스".to_owned())];

  assert_eq!(
    testing::answer_agreed(answer, written, Language::Ko, &agreed).as_deref(),
    Ok("아하스는 작업을 끝냈습니다.")
  );
  assert!(testing::answer(answer, written, Language::Ko).unwrap_err().contains("\"에이하스는\""));

  // A name's reading is not held to its spelling's letters, so the reading itself is agreed.
  assert_eq!(found_in_reading("에이하스", "아하스", Language::Ko), None);
}

#[test]
fn an_answer_that_left_a_word_out_is_asked_again_naming_the_word() {
  let written = "응답 시간은 1.2초, 오류율은 0.5%입니다.";
  let failed = testing::answer(
    r#"{"spoken": "응답 시간은 일쩌미초, 영쩌모퍼센트입니다.", "readings": []}"#,
    written,
    Language::Ko,
  )
  .unwrap_err();
  let finding = found(written, "응답 시간은 일쩌미초, 영쩌모퍼센트입니다.", Language::Ko).unwrap();
  let prompt = testing::prompt(Language::Ko, &[], written, Some(&finding));

  assert!(failed.contains("\"오류율은\""), "{failed}");
  assert!(prompt.contains(&finding.to_string()), "{prompt}");
}
