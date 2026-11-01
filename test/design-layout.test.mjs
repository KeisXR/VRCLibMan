import assert from "node:assert/strict";
import test from "node:test";
import { chromePath, withChromium } from "./helpers/chromium.mjs";

function contrast(first, second) {
  const luminance = (color) => color.match(/[\d.]+/g).slice(0, 3)
    .map(value => Number(value) / 255)
    .map(value => value <= .04045 ? value / 12.92 : ((value + .055) / 1.055) ** 2.4)
    .reduce((sum, value, i) => sum + value * [.2126, .7152, .0722][i], 0);
  const values = [luminance(first), luminance(second)].sort((a, b) => b - a);
  return (values[0] + .05) / (values[1] + .05);
}

// Render the real pages: selector text alone cannot catch inherited typography,
// native select heights, or padding accidentally applied to a fallback thumbnail.
test("design typography and corner geometry in Chromium", { timeout: 60000, skip: chromePath ? false : "Chrome not installed" }, async (t) => {
  await withChromium(async ({ baseUrl, page, evaluate, waitFor, pause }) => {
    await page("Page.navigate", { url: `${baseUrl}/test/fixtures/design-layout.html` });
    await waitFor("window.designFixture?.ready && !!document.querySelector('.avatar-card')");
    assert.equal(await evaluate("window.designFixture.error"), null);
    for (const theme of ["classic", "liquid"]) {
      await evaluate(`window.designFixture.api.updateSettings({ appearance: ${JSON.stringify(theme)} })`);
      for (const scheme of ["light", "dark"]) {
        await page("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: scheme }, { name: "prefers-reduced-motion", value: "reduce" }] });
        await t.test(`${theme}/${scheme}: title inheritance, thumbnails and bounded corners`, async () => {
          for (const width of [920, 1180, 1600]) {
            await page("Emulation.setDeviceMetricsOverride", { width, height: 1000, deviceScaleFactor: 1, mobile: false });
            await evaluate("location.hash = '#/'");
            await waitFor("document.querySelector('.avatar-card')");
            for (const density of ["density-compact", "density-roomy"]) {
              await evaluate(`document.querySelector('.content').className = 'content ${density}'`);
              const metrics = await evaluate(`(() => {
                const box = e => { const s=getComputedStyle(e),r=e.getBoundingClientRect(); return { width:r.width, height:r.height, font:parseFloat(s.fontSize), color:s.color, radius:parseFloat(s.borderTopLeftRadius), padding:s.padding, overflow:e.scrollWidth > e.clientWidth + 1 }; };
                return { text:getComputedStyle(document.body).color, cards:[...document.querySelectorAll('.avatar-card')].map(e => ({ ...box(e), title:box(e.querySelector('.card-title')), thumb:box(e.querySelector('.thumb')), content:box(e.querySelector('.card-title').parentElement) })), content:box(document.querySelector('.content')), toolbar:box(document.querySelector('.toolbar')) };
              })()`);
              assert.equal(metrics.content.overflow, false, `${width}/${density}: page stays inside its column`);
              assert.ok(metrics.toolbar.radius <= 18, "A wrapped toolbar must retain rectangular corners");
              for (const card of metrics.cards) {
                assert.equal(card.title.color, metrics.text, "Group heading colors must not dim card titles");
                assert.equal(card.title.font, 16, "The same card title size survives grouping and density changes");
                assert.equal(card.content.overflow, false, "Long Japanese names wrap inside the card");
                assert.ok(card.radius <= Math.min(20, card.height / 4), "Corner radius is proportional to the card");
                assert.equal(card.thumb.padding, "0px", "Fallback thumbnails must not receive text-column padding");
              }
              assert.equal(metrics.cards[0].thumb.width, metrics.cards[1].thumb.width);
              assert.equal(metrics.cards[0].thumb.height, metrics.cards[1].thumb.height, "Image and fallback occupy the same area");
            }
          }
        });
        await t.test(`${theme}/${scheme}: form control alignment and heading hierarchy`, async () => {
          for (const width of [920, 1180, 1600]) {
            await page("Emulation.setDeviceMetricsOverride", { width, height: 1000, deviceScaleFactor: 1, mobile: false });
            await evaluate("location.hash = '#/settings'");
            await waitFor("document.querySelector('#settings-design select')");
            const metrics = await evaluate(`(() => {
              const size = e => { const s=getComputedStyle(e),r=e.getBoundingClientRect(); return { height:r.height, font:s.fontSize, line:s.lineHeight, radius:parseFloat(s.borderTopLeftRadius), overflow:e.scrollWidth > e.clientWidth + 1 }; };
              const caption=document.createElement('span');caption.style.color='var(--faint)';document.body.append(caption);const captionColor=getComputedStyle(caption).color;caption.remove();
              return { captionColor, background:getComputedStyle(document.body).backgroundColor, controls:[...document.querySelectorAll('#settings-library .inline-control > *,#settings-design select')].map(size), help:[...document.querySelectorAll('.setting-copy > p')].map(e=>parseFloat(getComputedStyle(e).fontSize)), headings:[...document.querySelectorAll('.settings-section > header h2')].map(e=>parseFloat(getComputedStyle(e).fontSize)), subheadings:[...document.querySelectorAll('.settings-section h3')].map(e=>parseFloat(getComputedStyle(e).fontSize)), sections:[...document.querySelectorAll('.settings-section')].map(size) };
            })()`);
            assert.ok(Math.max(...metrics.controls.map(e => e.height)) - Math.min(...metrics.controls.map(e => e.height)) <= 1, "Text input, select and button share one height");
            assert.equal(new Set(metrics.controls.map(e => e.font)).size, 1);
            assert.equal(new Set(metrics.controls.map(e => e.line)).size, 1);
            assert.ok(metrics.controls.every(e => e.radius <= 12), "Text controls must not become capsules");
            assert.ok(metrics.help.every(size => size >= 13), "Descriptions are readable at normal scale");
            assert.ok(contrast(metrics.captionColor, metrics.background) >= 4.5, "Small captions stay legible on the page background");
            assert.ok(metrics.subheadings.every(size => size <= Math.min(...metrics.headings)), "Nested headings must not exceed section headings");
            assert.ok(metrics.sections.every(e => !e.overflow && e.radius <= 20));
            await evaluate("location.hash = '#/queue'");
            await waitFor("document.querySelector('.queue-card input[type=checkbox]')");
            const queue = await evaluate(`(() => {
              const checkbox=document.querySelector('.queue-card input[type=checkbox]').getBoundingClientRect();
              const row=document.querySelector('.queue-main'),thumb=row.querySelector('.thumb').getBoundingClientRect();
              return { checkbox:{width:checkbox.width,height:checkbox.height}, thumb:thumb.width, column:parseFloat(getComputedStyle(row).gridTemplateColumns) };
            })()`);
            assert.ok(queue.checkbox.width <= 20 && queue.checkbox.height <= 20, "Checkboxes must not inherit text input padding or minimum height");
            assert.ok(Math.abs(queue.thumb - queue.column) <= 1, "The compact thumbnail column matches its image width");
          }
          // A translated or long action can wrap without a pill outline squeezing its text.
          await evaluate("location.hash = '#/settings'");
          await waitFor("document.querySelector('#settings-design select')");
          const wrapped = await evaluate(`(() => {
            const button=document.querySelector('#settings-library .inline-control .button');
            button.style.width='140px'; button.style.whiteSpace='normal'; button.textContent='長い操作名を二行で表示しても文字が切れない';
            const r=button.getBoundingClientRect(),s=getComputedStyle(button),range=document.createRange();range.selectNodeContents(button);const text=range.getBoundingClientRect();
            return { height:r.height, textHeight:text.height, top:text.top-r.top, bottom:r.bottom-text.bottom, radius:parseFloat(s.borderTopLeftRadius) };
          })()`);
          assert.ok(wrapped.top >= 5 && wrapped.bottom >= 5, "Multiline control text retains padding above and below");
          assert.ok(wrapped.radius <= 12 && wrapped.height > 36);
          await evaluate("location.hash = '#/'");
          await waitFor("document.querySelector('.avatar-card')");
        });
      }
    }
  });
});
