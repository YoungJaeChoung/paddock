const { injectable, decorate } = require('@theia/core/shared/inversify');
const { KeyboardLayoutService } = require('@theia/core/lib/browser/keyboard/keyboard-layout-service');

// 글자 키 26개 중 이 수 이상이 라틴 글자를 내면 라틴 배열로 본다. 프랑스어(AZERTY)·독일어(QWERTZ)처럼 자리만 바뀐 배열도 라틴이다.
const N_LATIN_LETTERS_MIN = 13;

/**
 * 글자 키가 라틴 글자를 내는 키보드 배열인지 판정한다.
 *
 * 한글·러시아어처럼 글자 키가 라틴 글자를 내지 않는 배열에서는 Theia가 `ctrlcmd+b`를 "b를 내는 다른 조합"(맥 한글은 Option+B)으로
 * 바꿔 버려, 실제로 누른 Cmd+B(키 자리 기준)와 짝이 맞지 않는다. 그런 배열을 골라내는 데 쓴다.
 * 입력은 native-keymap의 `mapping`(`{ KeyA: { value: 'a', ... }, ... }`)이다. 비어 있거나 없으면 라틴으로 본다(변환이 원래 일어나지 않는다).
 *
 * Examples
 * --------
 * | 배열                  | KeyA·KeyB의 value | 결과  |
 * | --------------------- | ----------------- | ----- |
 * | 미국(US)              | `a`, `b`          | true  |
 * | 프랑스(AZERTY)        | `q`, `b`          | true  |
 * | 한글 두벌식           | `ㅁ`, `ㅠ`        | false |
 * | 러시아어              | `ф`, `и`          | false |
 * | `{}`·`undefined`      |                   | true  |
 */
function isLatinLayout(
    mapping,
) {
    const letterCodes = [...'ABCDEFGHIJKLMNOPQRSTUVWXYZ'].map(letter => `Key${letter}`);
    const n_present = letterCodes.filter(code => mapping?.[code]).length;
    const n_latin = letterCodes.filter(code => /^[a-z]$/i.test(mapping?.[code]?.value ?? '')).length;
    return n_present === 0 || n_latin >= N_LATIN_LETTERS_MIN;
}

/**
 * 라틴 글자를 내지 않는 배열에서는 단축키를 키 자리(미국 배열 기준)로 맞추는 키보드 배열 서비스.
 * 라틴 배열에서는 Theia 동작을 그대로 쓴다. 키 아래 표시 글자(`code2Character`)는 배열과 상관없이 그대로 남긴다.
 */
class PaddockKeyboardLayoutService extends KeyboardLayoutService {
    transformNativeLayout(
        nativeLayout,
    ) {
        const transformed = super.transformNativeLayout(nativeLayout);
        // 변환표를 비우면 resolveKeyCode가 단축키를 바꾸지 않아, Cmd+B가 누른 키 자리 그대로 meta+b로 맞는다.
        if (!isLatinLayout(nativeLayout?.mapping)) transformed.key2KeyCode = [];
        return transformed;
    }
}
decorate(injectable(), PaddockKeyboardLayoutService);

module.exports = { N_LATIN_LETTERS_MIN, isLatinLayout, PaddockKeyboardLayoutService };
