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
		{failed ? <p role="alert" className="p-4 font-protokoll text-sm">Filmen kunne ikke lastes. <a className="underline" href={full ? recording.raw : recording.video}>Åpne filmen direkte</a>.</p> : null}
	</div>;
}

export function ProductRecordingPlayer({ recordings }: { recordings: ProductRecording[] }) {
	const [selected, setSelected] = useState(0);
	const [full, setFull] = useState(false);
	const description = useId();
	const recording = recordings[selected];
	if (!recording) return null;
	return <div className="mx-auto grid w-full max-w-6xl gap-5" data-product-recordings>
		<div role="group" aria-label="Velg oppgave" className="flex flex-wrap gap-2">
			{recordings.map((item, index) => <button key={item.id} type="button" aria-pressed={selected === index}
				className={`min-h-11 rounded-full border px-5 py-2 font-protokoll text-sm focus-visible:outline-2 focus-visible:outline-offset-4 ${selected === index ? 'border-[#1d1d1b] bg-[#1d1d1b] text-white' : 'border-black/20 bg-transparent text-[#1d1d1b]'}`}
				onClick={() => { setSelected(index); setFull(false); }}>{item.label}</button>)}
		</div>
		<h3 className="font-arbeit text-3xl tracking-tight text-[#1d1d1b]">{recording.title}</h3>
		{/* Changing task or edit unmounts the previous video and pauses it. Never autoplay. */}
		<RecordingVideo key={`${recording.id}-${full}`} recording={recording} full={full} />
		<div className="flex flex-wrap items-start justify-between gap-4 font-protokoll text-sm leading-relaxed text-black/65">
			<p id={description} className="max-w-2xl">Fiktive eksempeldata. {full ? 'Hele oppgaven, uten tidskutt, inkludert oppfølgingen.' : recording.editingNote}</p>
			<button type="button" aria-pressed={full} aria-describedby={description} className="min-h-11 rounded-full border border-black/20 px-5 py-2 text-black focus-visible:outline-2 focus-visible:outline-offset-4"
				onClick={() => setFull(value => !value)}>{full ? 'Vis kortfilmen' : 'Se hele oppgaven'}</button>
		</div>
	</div>;
}
