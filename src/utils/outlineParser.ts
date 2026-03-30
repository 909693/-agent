export function parseOutlineToPlot(text: string) {
  const lines = text.split(/\r?\n/).map((l) => l.trim()).filter(Boolean);
  const acts: Array<{ number: number; title: string; theme: string; chapters: Array<{ number: number; title: string; summary: string; pov_character: string; plot_points: string[]; location: string; }> }> = [];
  let currentAct = { number: 1, title: "主线", theme: "", chapters: [] as any[] };
  let chapterCounter = 0;

  const actRegex = /^(第?[一二三四五六七八九十0-9]+卷|第?[一二三四五六七八九十0-9]+幕|卷[一二三四五六七八九十0-9]+|幕[一二三四五六七八九十0-9]+)[：:、\s-]*(.*)$/;
  const chapterRegex = /^(第?[一二三四五六七八九十百千0-9]+章)[：:、\s-]*(.*)$/;

  const flushAct = () => {
    if (currentAct.chapters.length > 0) acts.push(currentAct);
  };

  for (const line of lines) {
    const actMatch = line.match(actRegex);
    if (actMatch) {
      flushAct();
      currentAct = { number: acts.length + 1, title: actMatch[2] || actMatch[1], theme: "", chapters: [] };
      continue;
    }
    const chapterMatch = line.match(chapterRegex);
    if (chapterMatch) {
      chapterCounter += 1;
      currentAct.chapters.push({
        number: chapterCounter,
        title: chapterMatch[2] || chapterMatch[1],
        summary: "",
        pov_character: "",
        plot_points: [],
        location: "",
      });
      continue;
    }
    if (currentAct.chapters.length > 0) {
      const last = currentAct.chapters[currentAct.chapters.length - 1];
      last.summary = last.summary ? `${last.summary}\n${line}` : line;
    }
  }
  flushAct();

  return {
    acts,
    plot_points: [],
    subplots: [],
  };
}
