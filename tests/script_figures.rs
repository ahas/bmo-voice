//! The Korean date readings (docs/voice-plan.md, 「How figures are said」): a month and a day said as the reading
//! table gives them, their neighbours kept whole, and the same readings settled in the voice name table the spoken
//! words are derived with, so a month the model once said wrongly is said the same way next time.

use bmo_voice::script::figures::say_figures;
use bmo_voice::script::reading::{Language, say_names};
use bmo_voice::script::spoken::spoken_script;
use bmo_voice::store::voices;

/// The month and day spellings whose reading is settled, and what a Korean voice says.
const SETTLED: [(&str, &str); 9] = [
  ("1월", "일월"),
  ("10월", "시월"),
  ("11월", "십일월"),
  ("12월", "십이월"),
  ("1일", "일일"),
  ("5일", "오일"),
  ("11일", "십일일"),
  ("12일", "십이일"),
  ("21일", "이십일일"),
];

#[test]
fn the_month_and_day_readings_hold_their_neighbours() {
  for (written, said) in SETTLED {
    assert_eq!(say_figures(written), said, "{written}");
  }
}

#[test]
fn a_month_followed_by_a_day_is_two_words_across_a_space() {
  assert_eq!(say_figures("11월 5일"), "십일월 오일");
  assert_eq!(say_figures("10월 1일부터 11월 11일까지"), "시월 일일부터 십일월 십일일까지");
  assert_eq!(say_figures("12월 12일과 1월 21일"), "십이월 십이일과 일월 이십일일");
  assert_eq!(say_figures("2026-11-05"), "이천이십육년 십일월 오일");
}

#[test]
fn the_longer_spelling_is_read_whole_before_the_shorter_one() {
  let names = voices::seeds();

  assert_eq!(say_names("11월 5일", Language::Ko, &names), "십일월 오일");
  assert_eq!(say_names("1월 11일", Language::Ko, &names), "일월 십일일");
}

#[test]
fn the_ledger_settles_the_reading_the_rule_gives() {
  let seeds = voices::seeds();

  for (written, said) in SETTLED {
    let row = seeds
      .iter()
      .find(|name| name.written == written)
      .unwrap_or_else(|| panic!("the name table does not seed {written}"));

    assert_eq!(row.ko, said, "{written}");
    assert_eq!(row.ko, say_figures(written), "the ledger and the reading rule agree on {written}");
    assert!(row.en.is_empty(), "{written} is left as written in English");
  }
}

#[test]
fn a_briefing_line_keeps_every_word_the_reading_does_not_touch() {
  let before = "오늘 아침 경제 브리핑입니다. 시장은 어제보다 조금 안정을 되찾았고, 투자자들은 다음 주에 나올 \
    물가 지표와 금리 전망을 기다리고 있습니다. 지난주에는 여러 업종의 실적 발표가 이어졌고, 그 가운데 \
    몇몇 기업은 시장의 예상보다 나은 성적을 냈습니다. ";
  let after = " 발표된 지표 가운데 소비자 물가가 가장 눈에 띄었고, 시장의 반응도 잠잠했습니다. \
    다음 브리핑에서는 고용 지표와 함께 수출입 동향을 이어서 살펴보고, 각 업종의 흐름이 어떻게 달라지는지 \
    차분히 정리하겠습니다.";
  let line = format!("{before}11월 5일{after}");

  assert!(line.chars().count() > 200, "a briefing-length line");

  // Nothing outside the date is touched: the same words stand where they stood, none added, dropped or moved.
  assert_eq!(say_figures(&line), format!("{before}십일월 오일{after}"));
  assert_eq!(spoken_script(&line), line, "the written line reaches the derivation whole");
}
