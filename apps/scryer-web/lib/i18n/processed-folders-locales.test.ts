import assert from "node:assert/strict";
import test from "node:test";
import cs from "./locales/cs.ts";
import de from "./locales/de.ts";
import en from "./locales/en.ts";
import es from "./locales/es.ts";
import fr from "./locales/fr.ts";
import it from "./locales/it.ts";
import ja from "./locales/ja.ts";
import ko from "./locales/ko.ts";
import nl from "./locales/nl.ts";
import pt_BR from "./locales/pt_BR.ts";
import ru from "./locales/ru.ts";
import zh_CN from "./locales/zh_CN.ts";
import zh_HK from "./locales/zh_HK.ts";
import zh_TW from "./locales/zh_TW.ts";

const PREFIXES = [
  "settings.processedFolders.",
  "processedFolder.refusal.",
  "processedFolder.status.",
  "processedFolder.reason.",
  "processedFolder.action.",
];

const TRANSLATIONS: Record<string, Record<string, string>> = {
  cs, de, es, fr, it, ja, ko, nl, pt_BR, ru, zh_CN, zh_HK, zh_TW,
};

const isProcessedFolderKey = (key: string) => PREFIXES.some((prefix) => key.startsWith(prefix));
const placeholders = (text: string) => [...text.matchAll(/\{(\w+)\}/g)].map((match) => match[1]).sort();
const englishKeys = Object.keys(en).filter(isProcessedFolderKey);

test("English carries the processed-folder copy, including the copy-only notice", () => {
  assert.ok(englishKeys.length > 100, `only ${englishKeys.length} processed-folder keys in en`);
  assert.equal(
    en["settings.processedFolders.copyOnlyNotice"],
    "Files are always copied. Scryer never moves, links or deletes anything in this folder.",
  );
});

for (const [locale, dictionary] of Object.entries(TRANSLATIONS)) {
  test(`${locale} has every processed-folder key, translated, with the English placeholders`, () => {
    const missing = englishKeys.filter((key) => !dictionary[key]?.trim());
    assert.deepEqual(missing, [], `${locale} is missing processed-folder keys`);
    const extra = Object.keys(dictionary).filter((key) => isProcessedFolderKey(key) && !(key in en));
    assert.deepEqual(extra, [], `${locale} has processed-folder keys that en does not`);
    const mismatched = englishKeys.filter(
      (key) => placeholders(dictionary[key]).join(",") !== placeholders(en[key]).join(","),
    );
    assert.deepEqual(mismatched, [], `${locale} placeholders differ from en`);
    // Short labels can match English (a "Status" column), but the notice and
    // the longer sentences must be in the locale's own language.
    assert.notEqual(
      dictionary["settings.processedFolders.copyOnlyNotice"],
      en["settings.processedFolders.copyOnlyNotice"],
      `${locale} copy-only notice is still English`,
    );
    const untranslatedSentences = englishKeys.filter(
      (key) => en[key].length > 40 && dictionary[key] === en[key],
    );
    assert.deepEqual(untranslatedSentences, [], `${locale} sentences left in English`);
  });
}
