/**
 * Server-Timing parsing cases, vendored from the web platform tests at
 * `server-timing/resources/parsing/` (85 cases, generated there by
 * https://github.com/cvazac/generate-server-timing-tests).
 *
 * Each case is the header value an origin sent and the metrics a browser reads from it, so a
 * change to Faith's parse that a browser would not make fails here. The cases pin the grammar in
 * more depth than the Server-Timing standard's prose does: they are the reason the parse follows
 * that grammar rather than the standard's parsing algorithm, which 35 of them fail.
 */

module.exports = [
	{
		case: 0,
		header: "",
		metrics: [],
	},
	{
		case: 1,
		header: "metric",
		metrics: [
			{ name: "metric", duration: 0, description: "" },
		],
	},
	{
		case: 2,
		header: "metric;dur=123.4",
		metrics: [
			{ name: "metric", duration: 123.4, description: "" },
		],
	},
	{
		case: 3,
		header: "metric;dur=\"123.4\"",
		metrics: [
			{ name: "metric", duration: 123.4, description: "" },
		],
	},
	{
		case: 4,
		header: "metric;desc=description",
		metrics: [
			{ name: "metric", duration: 0, description: "description" },
		],
	},
	{
		case: 5,
		header: "metric;desc=\"description\"",
		metrics: [
			{ name: "metric", duration: 0, description: "description" },
		],
	},
	{
		case: 6,
		header: "metric;dur=123.4;desc=description",
		metrics: [
			{ name: "metric", duration: 123.4, description: "description" },
		],
	},
	{
		case: 7,
		header: "metric;desc=description;dur=123.4",
		metrics: [
			{ name: "metric", duration: 123.4, description: "description" },
		],
	},
	{
		case: 8,
		header: "aB3!#$%&'*+-.^_`|~",
		metrics: [
			{ name: "aB3!#$%&'*+-.^_`|~", duration: 0, description: "" },
		],
	},
	{
		case: 9,
		header: "metric;desc=\"descr;,=iption\";dur=123.4",
		metrics: [
			{ name: "metric", duration: 123.4, description: "descr;,=iption" },
		],
	},
	{
		case: 10,
		header: "metric ; ",
		metrics: [
			{ name: "metric", duration: 0, description: "" },
		],
	},
	{
		case: 11,
		header: "metric , ",
		metrics: [
			{ name: "metric", duration: 0, description: "" },
		],
	},
	{
		case: 12,
		header: "metric ; dur = 123.4 ; desc = description",
		metrics: [
			{ name: "metric", duration: 123.4, description: "description" },
		],
	},
	{
		case: 13,
		header: "metric ; desc = description ; dur = 123.4",
		metrics: [
			{ name: "metric", duration: 123.4, description: "description" },
		],
	},
	{
		case: 14,
		header: "metric;desc = \"description\"",
		metrics: [
			{ name: "metric", duration: 0, description: "description" },
		],
	},
	{
		case: 15,
		header: "metric1;dur=12.3;desc=description1,metric2;dur=45.6;desc=description2,metric3;dur=78.9;desc=description3",
		metrics: [
			{ name: "metric1", duration: 12.3, description: "description1" },
			{ name: "metric2", duration: 45.6, description: "description2" },
			{ name: "metric3", duration: 78.9, description: "description3" },
		],
	},
	{
		case: 16,
		header: "metric1,metric2 ,metric3, metric4 , metric5",
		metrics: [
			{ name: "metric1", duration: 0, description: "" },
			{ name: "metric2", duration: 0, description: "" },
			{ name: "metric3", duration: 0, description: "" },
			{ name: "metric4", duration: 0, description: "" },
			{ name: "metric5", duration: 0, description: "" },
		],
	},
	{
		case: 17,
		header: "metric;desc=\"description\"",
		metrics: [
			{ name: "metric", duration: 0, description: "description" },
		],
	},
	{
		case: 18,
		header: "metric;desc=\"\t description \t\"",
		metrics: [
			{ name: "metric", duration: 0, description: "\t description \t" },
		],
	},
	{
		case: 19,
		header: "metric;desc=\"descr\\\"iption\"",
		metrics: [
			{ name: "metric", duration: 0, description: "descr\"iption" },
		],
	},
	{
		case: 20,
		header: "metric;desc=\\",
		metrics: [
			{ name: "metric", duration: 0, description: "" },
		],
	},
	{
		case: 21,
		header: "metric;desc=\"",
		metrics: [
			{ name: "metric", duration: 0, description: "" },
		],
	},
	{
		case: 22,
		header: "metric;desc=\\\\",
		metrics: [
			{ name: "metric", duration: 0, description: "" },
		],
	},
	{
		case: 23,
		header: "metric;desc=\\\"",
		metrics: [
			{ name: "metric", duration: 0, description: "" },
		],
	},
	{
		case: 24,
		header: "metric;desc=\"\\",
		metrics: [
			{ name: "metric", duration: 0, description: "" },
		],
	},
	{
		case: 25,
		header: "metric;desc=\"\"",
		metrics: [
			{ name: "metric", duration: 0, description: "" },
		],
	},
	{
		case: 26,
		header: "metric;desc=\\\\\\",
		metrics: [
			{ name: "metric", duration: 0, description: "" },
		],
	},
	{
		case: 27,
		header: "metric;desc=\\\\\"",
		metrics: [
			{ name: "metric", duration: 0, description: "" },
		],
	},
	{
		case: 28,
		header: "metric;desc=\\\"\\",
		metrics: [
			{ name: "metric", duration: 0, description: "" },
		],
	},
	{
		case: 29,
		header: "metric;desc=\\\"\"",
		metrics: [
			{ name: "metric", duration: 0, description: "" },
		],
	},
	{
		case: 30,
		header: "metric;desc=\"\\\\",
		metrics: [
			{ name: "metric", duration: 0, description: "" },
		],
	},
	{
		case: 31,
		header: "metric;desc=\"\\\"",
		metrics: [
			{ name: "metric", duration: 0, description: "" },
		],
	},
	{
		case: 32,
		header: "metric;desc=\"\"\\",
		metrics: [
			{ name: "metric", duration: 0, description: "" },
		],
	},
	{
		case: 33,
		header: "metric;desc=\"\"\"",
		metrics: [
			{ name: "metric", duration: 0, description: "" },
		],
	},
	{
		case: 34,
		header: "metric;desc=\\\\\\\\",
		metrics: [
			{ name: "metric", duration: 0, description: "" },
		],
	},
	{
		case: 35,
		header: "metric;desc=\\\\\\\"",
		metrics: [
			{ name: "metric", duration: 0, description: "" },
		],
	},
	{
		case: 36,
		header: "metric;desc=\\\\\"\\",
		metrics: [
			{ name: "metric", duration: 0, description: "" },
		],
	},
	{
		case: 37,
		header: "metric;desc=\\\\\"\"",
		metrics: [
			{ name: "metric", duration: 0, description: "" },
		],
	},
	{
		case: 38,
		header: "metric;desc=\\\"\\\\",
		metrics: [
			{ name: "metric", duration: 0, description: "" },
		],
	},
	{
		case: 39,
		header: "metric;desc=\\\"\\\"",
		metrics: [
			{ name: "metric", duration: 0, description: "" },
		],
	},
	{
		case: 40,
		header: "metric;desc=\\\"\"\\",
		metrics: [
			{ name: "metric", duration: 0, description: "" },
		],
	},
	{
		case: 41,
		header: "metric;desc=\\\"\"\"",
		metrics: [
			{ name: "metric", duration: 0, description: "" },
		],
	},
	{
		case: 42,
		header: "metric;desc=\"\\\\\\",
		metrics: [
			{ name: "metric", duration: 0, description: "" },
		],
	},
	{
		case: 43,
		header: "metric;desc=\"\\\\\"",
		metrics: [
			{ name: "metric", duration: 0, description: "\\" },
		],
	},
	{
		case: 44,
		header: "metric;desc=\"\\\"\\",
		metrics: [
			{ name: "metric", duration: 0, description: "" },
		],
	},
	{
		case: 45,
		header: "metric;desc=\"\\\"\"",
		metrics: [
			{ name: "metric", duration: 0, description: "\"" },
		],
	},
	{
		case: 46,
		header: "metric;desc=\"\"\\\\",
		metrics: [
			{ name: "metric", duration: 0, description: "" },
		],
	},
	{
		case: 47,
		header: "metric;desc=\"\"\\\"",
		metrics: [
			{ name: "metric", duration: 0, description: "" },
		],
	},
	{
		case: 48,
		header: "metric;desc=\"\"\"\\",
		metrics: [
			{ name: "metric", duration: 0, description: "" },
		],
	},
	{
		case: 49,
		header: "metric;desc=\"\"\"\"",
		metrics: [
			{ name: "metric", duration: 0, description: "" },
		],
	},
	{
		case: 50,
		header: "metric;dur=12.3;desc=description1,metric;dur=45.6;desc=description2",
		metrics: [
			{ name: "metric", duration: 12.3, description: "description1" },
			{ name: "metric", duration: 45.6, description: "description2" },
		],
	},
	{
		case: 51,
		header: "metric;DuR=123.4;DeSc=description",
		metrics: [
			{ name: "metric", duration: 123.4, description: "description" },
		],
	},
	{
		case: 52,
		header: "MeTrIc;desc=DeScRiPtIoN",
		metrics: [
			{ name: "MeTrIc", duration: 0, description: "DeScRiPtIoN" },
		],
	},
	{
		case: 53,
		header: "metric;dur=foo",
		metrics: [
			{ name: "metric", duration: 0, description: "" },
		],
	},
	{
		case: 54,
		header: "metric;dur=\"foo\"",
		metrics: [
			{ name: "metric", duration: 0, description: "" },
		],
	},
	{
		case: 55,
		header: "metric1;foo=bar;desc=description;foo=bar;dur=123.4;foo=bar,metric2",
		metrics: [
			{ name: "metric1", duration: 123.4, description: "description" },
			{ name: "metric2", duration: 0, description: "" },
		],
	},
	{
		case: 56,
		header: "metric;dur=123.4;dur=567.8",
		metrics: [
			{ name: "metric", duration: 123.4, description: "" },
		],
	},
	{
		case: 57,
		header: "metric;dur=foo;dur=567.8",
		metrics: [
			{ name: "metric", duration: 0, description: "" },
		],
	},
	{
		case: 58,
		header: "metric;desc=description1;desc=description2",
		metrics: [
			{ name: "metric", duration: 0, description: "description1" },
		],
	},
	{
		case: 59,
		header: "metric;dur;dur=123.4;desc=description",
		metrics: [
			{ name: "metric", duration: 0, description: "description" },
		],
	},
	{
		case: 60,
		header: "metric;dur=;dur=123.4;desc=description",
		metrics: [
			{ name: "metric", duration: 0, description: "description" },
		],
	},
	{
		case: 61,
		header: "metric;desc;desc=description;dur=123.4",
		metrics: [
			{ name: "metric", duration: 123.4, description: "" },
		],
	},
	{
		case: 62,
		header: "metric;desc=;desc=description;dur=123.4",
		metrics: [
			{ name: "metric", duration: 123.4, description: "" },
		],
	},
	{
		case: 63,
		header: "metric;desc=d1 d2;dur=123.4",
		metrics: [
			{ name: "metric", duration: 123.4, description: "d1" },
		],
	},
	{
		case: 64,
		header: "metric1;desc=d1 d2,metric2",
		metrics: [
			{ name: "metric1", duration: 0, description: "d1" },
			{ name: "metric2", duration: 0, description: "" },
		],
	},
	{
		case: 65,
		header: "metric;desc=\"d1\" d2;dur=123.4",
		metrics: [
			{ name: "metric", duration: 123.4, description: "d1" },
		],
	},
	{
		case: 66,
		header: "metric1;desc=\"d1\" d2,metric2",
		metrics: [
			{ name: "metric1", duration: 0, description: "d1" },
			{ name: "metric2", duration: 0, description: "" },
		],
	},
	{
		case: 67,
		header: "metric==   \"\"foo;dur=123.4",
		metrics: [
			{ name: "metric", duration: 123.4, description: "" },
		],
	},
	{
		case: 68,
		header: "metric1==   \"\"foo,metric2",
		metrics: [
			{ name: "metric1", duration: 0, description: "" },
			{ name: "metric2", duration: 0, description: "" },
		],
	},
	{
		case: 69,
		header: "metric;dur foo=12",
		metrics: [
			{ name: "metric", duration: 0, description: "" },
		],
	},
	{
		case: 70,
		header: "metric;foo dur=12",
		metrics: [
			{ name: "metric", duration: 0, description: "" },
		],
	},
	{
		case: 71,
		header: " ",
		metrics: [],
	},
	{
		case: 72,
		header: "=",
		metrics: [],
	},
	{
		case: 73,
		header: "[",
		metrics: [],
	},
	{
		case: 74,
		header: "]",
		metrics: [],
	},
	{
		case: 75,
		header: ";",
		metrics: [],
	},
	{
		case: 76,
		header: ",",
		metrics: [],
	},
	{
		case: 77,
		header: "=;",
		metrics: [],
	},
	{
		case: 78,
		header: ";=",
		metrics: [],
	},
	{
		case: 79,
		header: "=,",
		metrics: [],
	},
	{
		case: 80,
		header: ",=",
		metrics: [],
	},
	{
		case: 81,
		header: ";,",
		metrics: [],
	},
	{
		case: 82,
		header: ",;",
		metrics: [],
	},
	{
		case: 83,
		header: "=;,",
		metrics: [],
	},
	{
		case: 84,
		header: "metric;\tdesc=\ttabs-should-get-trimmed\t;dur=\t42\t",
		metrics: [
			{ name: "metric", duration: 42, description: "tabs-should-get-trimmed" },
		],
	},
];
