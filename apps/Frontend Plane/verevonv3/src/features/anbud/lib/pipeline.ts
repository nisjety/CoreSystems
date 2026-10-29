import { Clock3, EyeOff, FilePenLine, Inbox, Send } from 'lucide-solid'

export const anbudStages = [
  { id: 'inbox', no: 'Innboks', en: 'Inbox', icon: Inbox },
  { id: 'review', no: 'Vurderes', en: 'Considering', icon: Clock3 },
  { id: 'working', no: 'Under arbeid', en: 'In progress', icon: FilePenLine },
  { id: 'sent', no: 'Levert', en: 'Submitted', icon: Send },
  { id: 'hidden', no: 'Skjult', en: 'Hidden', icon: EyeOff },
]
