"use client";

import { useEffect, useId, useRef, useState } from 'react';
import type { ProductRecording } from '@/lib/product-recording-contract';

function RecordingVideo({ recording, full }: { recording: ProductRecording; full: boolean }) {
	const ref = useRef<HTMLVideoElement>(null);
	const [failed, setFailed] = useState(false);
	useEffect(() => {
		const video = ref.current;
		if (!video) return;
		const pause = () => { if (document.hidden) video.pause(); };
		const observer = new IntersectionObserver(([entry]) => { if (!entry.isIntersecting) video.pause(); });
		observer.observe(video);
		document.addEventListener('visibilitychange', pause);
		return () => { video.pause(); observer.disconnect(); document.removeEventListener('visibilitychange', pause); };
	}, []);
	return <div className="overflow-hidden rounded-2xl border border-black/10 bg-[#f4f3f0]">
		<video ref={ref} className="aspect-video w-full object-contain" controls playsInline preload="none" poster={recording.poster}
			aria-label={`${recording.label} — ${full ? 'hele oppgaven' : 'kortfilm'}`}
			src={full ? recording.raw : recording.video} onError={() => setFailed(true)}>
			<track kind="captions" label="Norsk" srcLang="nb" src={full ? recording.rawCaptions : recording.captions} default />
		</video>
		{failed ? <p role="alert" className="p-4 font-geist text-sm">Filmen kunne ikke lastes. <a className="underline" href={full ? recording.raw : recording.video}>Åpne filmen direkte</a>.</p> : null}
	</div>;
}

export function ProductRecordingPlayer({ recordings }: { recordings: ProductRecording[] }) {
	const [selected, setSelected] = useState(0);
	const [full, setFull] = useState(false);
	const description = useId();
	const transcriptIntro = useId();
	const recording = recordings[selected];
	if (!recording) return null;
	const transcript = full ? recording.rawTranscript : recording.transcript;
	return <div className="mx-auto grid w-full max-w-6xl gap-5" data-product-recordings>
		<div role="group" aria-label="Velg oppgave" className="flex flex-wrap gap-2">
			{recordings.map((item, index) => <button key={item.id} type="button" aria-pressed={selected === index}
				className={`min-h-11 rounded-full border px-5 py-2 font-geist text-sm focus-visible:outline-2 focus-visible:outline-offset-4 ${selected === index ? 'border-[#1d1d1b] bg-[#1d1d1b] text-white' : 'border-black/20 bg-transparent text-[#1d1d1b]'}`}
				onClick={() => { setSelected(index); setFull(false); }}>{item.label}</button>)}
		</div>
		<h3 className="font-manrope text-3xl tracking-tight text-[#1d1d1b]">{recording.title}</h3>
		{/* Changing task or edit unmounts the previous video and pauses it. Never autoplay. */}
		<RecordingVideo key={`${recording.id}-${full}`} recording={recording} full={full} />
		<div className="flex flex-wrap items-start justify-between gap-4 font-geist text-sm leading-relaxed text-black/65">
			<p id={description} className="max-w-2xl">Fiktive eksempeldata. {full ? 'Hele oppgaven, uten tidskutt, inkludert oppfølgingen.' : recording.editingNote}</p>
			<button type="button" aria-pressed={full} aria-describedby={description} className="min-h-11 rounded-full border border-black/20 px-5 py-2 text-black focus-visible:outline-2 focus-visible:outline-offset-4"
				onClick={() => setFull(value => !value)}>{full ? 'Vis kortfilmen' : 'Se hele oppgaven'}</button>
		</div>
		{/* The films have no speech, so a captions track alone gives a screen-reader user no equivalent. The cue texts are the text alternative of whichever film is shown. A native <details> is collapsed until asked for, is keyboard operable without script and keeps its open state when the film changes. */}
		<details data-recording-transcript className="rounded-2xl border border-black/10 px-5 font-geist text-sm leading-relaxed text-black/65">
			<summary className="cursor-pointer py-3 text-[#1d1d1b] focus-visible:outline-2 focus-visible:outline-offset-4">Tekstalternativ</summary>
			<p id={transcriptIntro} className="max-w-2xl">{recording.label}, {full ? 'hele oppgaven' : 'kortfilmen'}. Filmen har ingen tale. Her er handlingen beskrevet steg for steg.</p>
			<ol aria-labelledby={transcriptIntro} className="my-3 max-w-2xl list-decimal space-y-2 pb-1 pl-6">{transcript.map((cue, index) => <li key={index}>{cue}</li>)}</ol>
		</details>
	</div>;
}
