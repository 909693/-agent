import { useEffect, useRef, useState } from "react";

interface Genre {
  id: string;
  name: string;
  description: string;
  promptHint: string;
  isDefault: boolean;
}

const STORAGE_KEY = "retl_genres";

const DEFAULT_GENRES: Genre[] = [
  {
    id: "g1", name: "玄幻/仙侠",
    description: "以东方神话体系为根基，融合道家修真、佛家禅意与上古神话元素。主角通过修炼突破境界、追求长生大道，世界观宏大，涵盖凡人界、仙界、魔界等多重位面。读者期待热血升级、宗门争霸、天道对抗与逆天改命的爽感。",
    promptHint: "世界观构建：设定完整的修炼境界体系（如炼气→筑基→金丹→元婴→化神→渡劫→大乘），每个境界有明确的实力表现和突破条件；构建宗门/家族势力格局，设定灵石经济体系和天材地宝稀缺资源。角色类型：主角多为天赋异禀或逆境崛起的修士，配角包括忠诚道侣、亦敌亦友的天骄、深不可测的老怪物。核心冲突：修炼资源争夺、宗门存亡、天道不公、因果轮回。叙事风格：热血燃向为主，穿插悟道哲思，战斗场面要有法术视觉化描写（剑光、雷劫、领域展开），善用'以小博大'和'绝境逆转'的爽点设计。",
    isDefault: true
  },
  {
    id: "g2", name: "科幻",
    description: "以科学理论或技术推演为基础，探索未来社会、星际文明、人工智能、时间旅行等主题。硬科幻注重科学逻辑自洽，软科幻侧重社会人文思考。读者期待脑洞大开的设定、严谨的世界观和对人类命运的深层思考。",
    promptHint: "世界观构建：根据硬/软科幻方向选择核心科技设定（FTL超光速引擎、戴森球、脑机接口、基因编辑、量子纠缠通讯等），技术细节要自洽可信；构建未来社会结构（星际联邦、企业殖民、AI治理、赛博朋克阶层分化）。角色类型：科学家、星舰舰长、AI觉醒体、基因改造人、太空拓荒者、赛博朋克黑客。核心冲突：人与技术的边界、文明等级碰撞、AI伦理困境、资源枯竭与星际殖民、时间悖论。叙事风格：冷峻理性的笔触，用精确的技术描写营造真实感，在宏大的宇宙尺度下探讨人性本质，战斗场面侧重太空战术、舰队阵型和科技武器的视觉呈现。",
    isDefault: true
  },
  {
    id: "g3", name: "都市",
    description: "以当代城市生活为背景，涵盖职场商战、都市异能、重生逆袭、豪门恩怨等子类型。贴近现实生活但高于现实，满足读者对成功、逆袭和掌控命运的幻想。读者期待代入感强的现代场景、痛快的打脸逆袭和步步高升的成就感。",
    promptHint: "世界观构建：以真实都市为蓝本，根据子类型叠加特殊设定——都市异能需设定能力体系和隐秘组织，商战文需构建行业生态和商业逻辑，重生文需明确金手指规则和蝴蝶效应边界。角色类型：隐忍蛰伏的主角、嚣张跋扈的反派富二代、外冷内热的女主、深藏不露的老前辈、忠诚的兄弟团。核心冲突：阶层跨越、商业博弈、家族恩怨、都市暗面势力、感情纠葛。叙事风格：节奏明快，对话要接地气有网感，打脸情节要铺垫充分、反转痛快；商战描写要有专业感但不枯燥；都市场景描写要有烟火气和时代感，善用品牌、地标、流行文化增强代入感。",
    isDefault: true
  },
  {
    id: "g4", name: "言情",
    description: "以男女主角的情感发展为核心主线，涵盖甜宠、虐恋、破镜重圆、先婚后爱等多种模式。情感描写细腻深入，注重心理刻画和关系递进。读者期待心动的恋爱体验、丰满的人物塑造和令人满足的情感归宿（HE为主流）。",
    promptHint: "世界观构建：根据背景设定（古代/现代/架空）构建合理的社会环境和婚恋观念，为感情线提供外部压力和推动力。角色类型：男主常见人设——冷面总裁、温润如玉、病娇偏执、忠犬系、禁欲系；女主常见人设——独立飒爽、软萌可爱、外柔内刚、毒舌学霸；配角要有CP感或制造误会的功能性。核心冲突：身份差距、误会与信任危机、第三者介入、家族反对、前任纠缠、性格磨合。叙事风格：情感描写要细腻但不矫情，善用'推拉感'制造心动——靠近又退缩、误会又和好、暧昧又克制；对话要有CP感和化学反应；甜宠文要密集撒糖，虐文要虐得有逻辑、甜得有铺垫；注重女性视角的情感体验和内心独白。",
    isDefault: true
  },
  {
    id: "g5", name: "悬疑",
    description: "以谜团、推理和真相揭示为核心驱动力，涵盖本格推理、社会派推理、犯罪悬疑、心理惊悚等子类型。逻辑严密，线索公平，注重智力博弈和反转设计。读者期待烧脑的推理过程、意想不到的真相和正义得到伸张的满足感。",
    promptHint: "世界观构建：根据子类型选择设定——本格推理需要密室/不在场证明等经典诡计，社会派需要真实的社会背景和人性动机，心理惊悚需要不可靠叙述和认知颠覆。角色类型：天才侦探（但要有人性弱点）、高智商犯罪者（动机要有说服力）、关键证人、误导性嫌疑人、暗线操控者。核心冲突：真相与谎言、正义与法律的灰色地带、信任与背叛、过去的罪与现在的罚。叙事风格：信息投放要精准——公平地给出所有关键线索但巧妙隐藏在细节中；节奏要紧凑，每章结尾留钩子；善用多视角叙事制造信息差；反转要有至少两层——第一层反转让读者惊讶，第二层反转让读者回味；推理过程要展现逻辑链条，让读者有参与感。",
    isDefault: true
  },
  {
    id: "g6", name: "历史",
    description: "以真实历史朝代为背景，在史实框架内虚构人物和情节。要求对历史时期的政治制度、社会风貌、文化习俗有准确把握，在'大事不虚、小事不拘'的原则下展开故事。读者期待厚重的历史质感、权谋博弈的智慧和以古鉴今的思考。",
    promptHint: "世界观构建：选定具体朝代和历史时期，准确还原该时期的官制、军制、经济制度、社会阶层、衣食住行等细节；重大历史事件作为故事背景板，虚构人物在历史缝隙中活动。角色类型：权谋型主角（帝王将相或幕后谋士）、乱世枭雄、忠臣良将、才女名妓、市井百姓；历史真实人物出场时要符合史料记载的性格特征。核心冲突：朝堂权力斗争、边疆战事、改革与守旧、民族融合与冲突、个人命运与历史洪流。叙事风格：文风要有历史厚重感，用词典雅但不晦涩；对话要符合时代语境（避免现代网络用语）；政治博弈要写出'棋局感'——每一步都有深意；战争场面要有战略纵深；善用历史典故和诗词增添文化底蕴。",
    isDefault: true
  },
  {
    id: "g7", name: "恐怖",
    description: "以制造恐惧、惊悚和不安为核心目标，涵盖灵异鬼怪、克苏鲁宇宙恐怖、心理恐怖、民俗怪谈等子类型。通过未知、反常和失控感触发读者的原始恐惧。读者期待脊背发凉的沉浸体验、精心设计的恐怖桥段和出人意料的真相。",
    promptHint: "世界观构建：根据子类型选择恐怖源——灵异类需设定鬼魂规则和驱邪体系，克苏鲁类需构建不可名状的宇宙恐怖和SAN值机制，心理恐怖需设计认知陷阱和现实扭曲，民俗类需融入真实民间传说和禁忌习俗。角色类型：普通人被卷入超自然事件（代入感强）、不信邪的探索者、知情但无法言说的守秘人、已经疯狂的前任调查者。核心冲突：已知与未知的边界、理性与疯狂的拉锯、生存与好奇的矛盾、过去的罪孽与现在的报应。叙事风格：氛围营造重于直接惊吓——'少即是多'，暗示比展示更恐怖；善用五感描写制造不安（不该存在的声音、黑暗中的触感、腐败的气味）；节奏要有'呼吸感'——紧张后短暂放松再突然收紧；信息要有'缺失感'——永远有解释不了的细节。",
    isDefault: true
  },
  {
    id: "g8", name: "武侠",
    description: "以古代中国为背景，讲述江湖儿女的恩怨情仇和侠义精神。核心元素包括武功体系、门派纷争、江湖规矩和侠之大者的精神追求。区别于仙侠的超自然力量，武侠更注重人体武学极限和人间烟火。读者期待快意恩仇的江湖体验和'侠之大者，为国为民'的精神共鸣。",
    promptHint: "世界观构建：设定武功体系（内功心法、外功招式、轻功暗器），武学境界要有上限且基于人体极限；构建江湖势力格局（名门正派、邪教魔门、朝廷鹰犬、绿林草莽），设定武林规矩和江湖道义。角色类型：侠客（正义但不完美）、枭雄（有魅力的反派）、隐世高人、江湖浪子、侠女/女中豪杰、市井小人物。核心冲突：正邪之争、门派恩怨、武林秘籍争夺、朝廷与江湖的博弈、个人情义与大义的抉择。叙事风格：文风要有古典韵味，善用武侠特有的意象（大漠孤烟、古道西风、酒馆说书、雨夜追杀）；武打描写要写意——重意境轻招式，用速度感和力量感传达武学境界；对话要有江湖气（豪迈、义气、恩怨分明）；情感线要含蓄深沉，'执手相看泪眼'胜过千言万语。",
    isDefault: true
  },
  {
    id: "g9", name: "游戏",
    description: "以游戏世界或游戏化现实为舞台，融合RPG等级系统、技能树、装备强化、副本挑战等游戏机制。涵盖虚拟网游、异世界GameLit、无限流等子类型。读者期待清晰的成长数据、策略性的战斗和不断解锁新内容的探索乐趣。",
    promptHint: "世界观构建：设定完整的游戏系统——等级上限、经验获取方式、职业分类（战士/法师/盗贼/牧师等）、技能树分支、装备品质体系（白绿蓝紫橙红）、副本难度分级、公会系统和PVP规则；如果是穿越到游戏世界，需明确NPC是否有自我意识、死亡惩罚机制。角色类型：利用系统漏洞或独特理解获得优势的主角、竞争对手玩家、NPC伙伴（可能觉醒自我意识）、游戏管理者/GM、隐藏BOSS。核心冲突：排行榜竞争、公会战争、隐藏任务线、系统BUG引发的危机、虚拟与现实的边界模糊。叙事风格：战斗描写要融入数据面板（伤害数字、技能冷却、BUFF/DEBUFF状态），但不能变成枯燥的数据罗列；善用游戏玩家熟悉的术语和梗增强代入感；升级和获得新装备时要有'开箱'般的爽感；策略性要强——主角靠智慧而非单纯数值碾压取胜。",
    isDefault: true
  },
  {
    id: "g10", name: "末日",
    description: "以文明崩塌后的生存挑战为核心，涵盖丧尸末日、核战废土、病毒瘟疫、异变入侵等子类型。在极端环境下考验人性，探讨文明重建的可能性。读者期待紧张刺激的生存体验、资源管理的策略感和人性在绝境中的真实展现。",
    promptHint: "世界观构建：明确末日成因（丧尸病毒、核战辐射、外星入侵、灵气复苏、AI叛变等）及其对世界的具体改变；设定生存规则——食物水源获取、安全区域分布、威胁等级划分、幸存者社会结构（军事化据点、自由贸易镇、掠夺者团伙）；如有变异/进化体系需设定清晰规则。角色类型：务实冷静的生存者主角、道德底线不同的幸存者群体、失去人性的掠夺者、试图重建秩序的领袖、科学家（寻找解药/真相）。核心冲突：生存资源争夺、人与人的信任危机、道德困境（牺牲少数救多数）、文明重建vs弱肉强食、末日真相的追寻。叙事风格：基调压抑但不绝望，在黑暗中保留人性的微光；生存细节要具体真实（食物保质期、武器维护、伤口处理）；战斗场面要有生死一线的紧迫感；善用'安全感的崩塌'制造张力——刚建立的据点被攻破、信任的同伴背叛；人物成长要体现在生存智慧和心理韧性上。",
    isDefault: true
  },
];

const GENRES_VERSION = "v2"; // bump to force refresh

function loadGenres(): Genre[] {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    const storedList: Genre[] = stored ? JSON.parse(stored) : [];
    const ver = localStorage.getItem(STORAGE_KEY + "_ver");
    if (ver === GENRES_VERSION) {
      if (stored) return storedList;
    } else if (Array.isArray(storedList) && storedList.length > 0) {
      // Version bump: MERGE defaults with the user's own genres instead of
      // overwriting — otherwise every user-created genre is wiped.
      const defaultIds = new Set(DEFAULT_GENRES.map((g) => g.id));
      const userGenres = storedList.filter((g) => g && !defaultIds.has(g.id));
      const merged = [...DEFAULT_GENRES, ...userGenres];
      localStorage.setItem(STORAGE_KEY, JSON.stringify(merged));
      localStorage.setItem(STORAGE_KEY + "_ver", GENRES_VERSION);
      return merged;
    }
  } catch { /* ignore */ }
  localStorage.setItem(STORAGE_KEY, JSON.stringify(DEFAULT_GENRES));
  localStorage.setItem(STORAGE_KEY + "_ver", GENRES_VERSION);
  return [...DEFAULT_GENRES];
}

function saveGenres(genres: Genre[]) {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(genres));
}

// Built-in genre keys (used by CreateProjectDialog / ChatCreator) → a substring
// of the matching genre name, so a project's `genre` resolves to a genre guide.
const GENRE_KEY_ALIASES: Record<string, string> = {
  fantasy: "玄幻", scifi: "科幻", urban: "都市", romance: "言情",
  mystery: "悬疑", history: "历史", horror: "恐怖",
};

/** Resolve a project's `genre` (a built-in key like "fantasy", or a custom
 *  genre's id/name) to its authoring guide (promptHint); "" if none matches. */
export function getGenrePromptHint(genre: string): string {
  if (!genre) return "";
  const genres = loadGenres();
  const alias = GENRE_KEY_ALIASES[genre] || genre;
  const match = genres.find((g) =>
    g.id === genre || g.name === genre || g.name.includes(alias) || alias.includes(g.name)
  );
  return match?.promptHint?.trim() || "";
}

export function GenreManager() {
  const [genres, setGenres] = useState<Genre[]>(loadGenres);
  const [editing, setEditing] = useState<Genre | null>(null);
  const [showForm, setShowForm] = useState(false);
  const [form, setForm] = useState({ name: "", description: "", promptHint: "" });
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const formRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (showForm) formRef.current?.scrollIntoView({ behavior: "smooth", block: "nearest" });
  }, [showForm, editing]);

  const toggleExpand = (id: string) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id); else next.add(id);
      return next;
    });
  };

  const handleSave = () => {
    if (!form.name.trim()) return;
    let updated: Genre[];
    if (editing) {
      updated = genres.map((g) => (g.id === editing.id ? { ...g, ...form } : g));
    } else {
      updated = [...genres, { id: Date.now().toString(), ...form, isDefault: false }];
    }
    setGenres(updated);
    saveGenres(updated);
    setShowForm(false);
    setEditing(null);
    setForm({ name: "", description: "", promptHint: "" });
  };

  const handleDelete = (g: Genre) => {
    if (!window.confirm(`确定删除类型「${g.name}」？删除后无法恢复。`)) return;
    const updated = genres.filter((x) => x.id !== g.id);
    setGenres(updated);
    saveGenres(updated);
  };

  const handleEdit = (g: Genre) => {
    setEditing(g);
    setForm({ name: g.name, description: g.description, promptHint: g.promptHint });
    setShowForm(true);
  };

  return (
    <div>
      <div className="page-header">
        <h2>小说类型管理</h2>
        <button className="btn-primary" onClick={() => { setEditing(null); setForm({ name: "", description: "", promptHint: "" }); setShowForm(true); }}>
          + 新增类型
        </button>
      </div>
      {showForm && (
        <div className="form-card" ref={formRef}>
          <h3>{editing ? "编辑类型" : "新增类型"}</h3>
          <div className="form-stack">
            <label className="form-field">
              类型名称 *
              <input placeholder="例如：西方奇幻" value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} />
            </label>
            <label className="form-field">
              简要描述
              <textarea placeholder="一两句话说明这个类型的题材特点和读者期待" rows={2} value={form.description}
                onChange={(e) => setForm({ ...form, description: e.target.value })} />
            </label>
            <label className="form-field">
              AI 创作指导
              <textarea placeholder="告诉 AI 这个类型该如何构建世界观、塑造角色、设计核心冲突与叙事风格。生成框架和章节时会注入提示词。" rows={7} value={form.promptHint}
                onChange={(e) => setForm({ ...form, promptHint: e.target.value })} />
            </label>
            <div className="toolbar-actions">
              <button className="btn-primary" onClick={handleSave} disabled={!form.name.trim()}>保存</button>
              <button className="btn-outline" onClick={() => { setShowForm(false); setEditing(null); }}>取消</button>
            </div>
          </div>
        </div>
      )}

      <table className="chapter-table genre-table">
        <thead>
          <tr>
            <th>类型</th>
            <th>描述</th>
            <th>AI 提示</th>
            <th>操作</th>
          </tr>
        </thead>
        <tbody>
          {genres.map((g) => {
            const isOpen = expanded.has(g.id);
            return (
              <tr key={g.id}>
                <td className="genre-name-cell">{g.name} {g.isDefault && <span className="tag">默认</span>}</td>
                <td className="genre-desc-cell">
                  <div className={`clamp-text${isOpen ? " expanded" : ""}`}>{g.description}</div>
                </td>
                <td className="genre-hint-cell">
                  <div className={`clamp-text${isOpen ? " expanded" : ""}`}>{g.promptHint}</div>
                  {(g.promptHint.length > 100 || g.description.length > 100) && (
                    <button className="clamp-toggle" onClick={() => toggleExpand(g.id)}>
                      {isOpen ? "收起" : "展开全文"}
                    </button>
                  )}
                </td>
                <td>
                  <div className="toolbar-actions" style={{ gap: 6 }}>
                    <button className="btn-sm" onClick={() => handleEdit(g)}>编辑</button>
                    {!g.isDefault && (
                      <button className="btn-sm" style={{ borderColor: "var(--danger)", color: "var(--danger)" }}
                        onClick={() => handleDelete(g)}>删除</button>
                    )}
                  </div>
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
