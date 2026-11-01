//! M1 interactive desktop Terminal input, designed for events copied from a
//! trusted kernel keyboard/IPC broker. No direct port I/O, pointer access or
//! privileged framebuffer writes. Each completed line is a separate command.
pub const MAX_LINE:usize=192;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Error{Invalid,Full,TooSmall}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Event{Typed,Erased,Submitted(usize),Empty}

/// Live per-process editable command line. Uses the same bounded cursor
/// editor as the existing standalone line-editing tests: no second source
/// buffer, dynamic allocation or privileged input pointers.
#[derive(Clone,Copy)]
struct HistoryEntry{bytes:[u8;MAX_LINE],len:usize}
pub const HISTORY_LIMIT:usize=4;
pub struct InputLine {
 editor:LineEditor,
 clipboard:[u8;MAX_LINE],
 clipboard_len:usize,
 history:[Option<HistoryEntry>;HISTORY_LIMIT],
 history_len:usize,
 history_cursor:Option<usize>,
 history_draft:[u8;MAX_LINE],
 history_draft_len:usize,
}
impl InputLine{
 pub const fn new()->Self{
  Self{editor:LineEditor::new(),clipboard:[0;MAX_LINE],clipboard_len:0,
   history:[None;HISTORY_LIMIT],history_len:0,history_cursor:None,
   history_draft:[0;MAX_LINE],history_draft_len:0}
 }
 pub fn length(&self)->usize{self.editor.bytes().len()}
 pub fn bytes(&self)->&[u8]{self.editor.bytes()}
 pub fn caret(&self)->usize{self.editor.caret()}
 pub fn is_empty(&self)->bool{self.length()==0}
 pub fn clipboard_bytes(&self)->&[u8]{&self.clipboard[..self.clipboard_len]}
 /// Reset an executed draft without destroying the authenticated session's
 /// clipboard, which can be pasted into a subsequent command.
 pub fn reset(&mut self){
  self.editor.clear();self.history_cursor=None;
  self.history_draft.fill(0);self.history_draft_len=0;
 }
 /// On crash/restart/exit erase all process-private command/clipboard bytes.
 pub fn forget(&mut self){
  self.reset();self.clipboard.fill(0);self.clipboard_len=0;
  self.history.fill(None);self.history_len=0;
 }
 pub fn clear_line(&mut self){self.reset();}
 pub fn kill_line(&mut self)->Event{
  if self.is_empty(){return Event::Empty;}
  self.reset();Event::Erased
 }
 pub fn restore_if_empty(&mut self,source:&[u8])->Result<(),Error>{
  if !self.is_empty(){return Err(Error::Invalid);}
  self.set_line(source)
 }
 /// Erase the word immediately before the CURRENT caret.
 pub fn erase_word(&mut self)->Event{self.editor.erase_previous_word()}
 pub fn set_line(&mut self,line:&[u8])->Result<(),Error>{
  self.editor.replace_line(line)
 }
 /// Kernel-only navigation API; submitting a line is permitted ONLY
 /// through feed() with a user-issued Enter, never a navigation event.
 pub fn action(&mut self,key:EditKey)->Result<Event,Error>{
  if key==EditKey::Submit{return Err(Error::Invalid);}
  self.editor.edit(key,&mut[])
 }
 pub fn word_left(&mut self)->usize{self.editor.previous_word()}
 pub fn word_right(&mut self)->usize{self.editor.next_word()}
 pub fn delete_to_end(&mut self)->Event{
  let at=self.editor.caret;
  if at==self.editor.len{return Event::Empty;}
  self.editor.line[at..self.editor.len].fill(0);
  self.editor.len=at;Event::Erased
 }
 /// Atomic validated selection replacement, including overlap and capacity.
 pub fn replace_range(&mut self,start:usize,end:usize,next:&[u8])
  ->Result<Event,Error>{
  self.editor.replace_range(start,end,next)
 }
 /// One atomic literal replacement across the current command draft.
 /// This cannot read files, send syscalls, or overwrite the clipboard.
 pub fn replace_all_literal(&mut self,needle:&[u8],replacement:&[u8])
  ->Result<usize,Error>{
  self.editor.replace_all_literal(needle,replacement)
 }
 /// A keyboard source may paste only bounded printable ASCII; all-or-nothing.
 pub fn paste(&mut self,text:&[u8])->Result<(),Error>{self.editor.paste(text)}
 /// Store a selection only after range/capacity preflight. Previous clipboard
 /// survives invalid operations and short/malformed selection.
 pub fn copy_range(&mut self,start:usize,end:usize)->Result<usize,Error>{
  if start>=end{return Err(Error::Invalid);}
  let mut data=[0u8;MAX_LINE];
  let count=self.editor.copy_range(start,end,&mut data)?;
  self.clipboard.fill(0);self.clipboard[..count].copy_from_slice(&data[..count]);
  self.clipboard_len=count;Ok(count)
 }
 pub fn cut_range(&mut self,start:usize,end:usize)->Result<usize,Error>{
  if start>=end{return Err(Error::Invalid);}
  if end>self.length(){return Err(Error::Invalid);}
  let len=self.copy_range(start,end)?;
  self.editor.replace_range(start,end,b"")?;
  Ok(len)
 }
 pub fn paste_clipboard(&mut self)->Result<Event,Error>{
  if self.clipboard_len==0{return Ok(Event::Empty);}
  self.editor.paste(&self.clipboard[..self.clipboard_len])?;
  Ok(Event::Typed)
 }
 /// Store only a successfully submitted command, never a failed/partial
 /// draft. Adjacent duplicate commands are coalesced. Oldest commands
 /// expire first and never survive process forget/teardown.
 fn remember(&mut self,command:&[u8]){
  if command.is_empty(){return;}
  if self.history_len>0{
   if let Some(last)=self.history[self.history_len-1]{
    if &last.bytes[..last.len]==command{return;}
   }
  }
  let mut entry=HistoryEntry{bytes:[0;MAX_LINE],len:command.len()};
  entry.bytes[..command.len()].copy_from_slice(command);
  if self.history_len==HISTORY_LIMIT{
   for i in 1..HISTORY_LIMIT{self.history[i-1]=self.history[i];}
   self.history[HISTORY_LIMIT-1]=Some(entry);
  }else{
   self.history[self.history_len]=Some(entry);
   self.history_len+=1;
  }
 }
 pub fn history_count(&self)->usize{self.history_len}
 /// Navigate from a current, potentially unsaved draft toward older
 /// successfully submitted commands. The draft is restored on navigation
 /// forward past the newest entry.
 pub fn history_previous(&mut self)->Result<usize,Error>{
  if self.history_len==0{return Err(Error::Invalid);}
  let next=match self.history_cursor{
   Some(index)=>index.saturating_sub(1),
   None=>{
    self.history_draft.fill(0);
    self.history_draft_len=self.length();
    self.history_draft[..self.history_draft_len]
     .copy_from_slice(self.editor.bytes());
    self.history_len-1
   }
  };
  let entry=self.history[next].ok_or(Error::Invalid)?;
  self.editor.replace_line(&entry.bytes[..entry.len])?;
  self.history_cursor=Some(next);
  Ok(entry.len)
 }
 /// Reverse-search prior successfully submitted commands by a printable
 /// prefix. Success loads the newest matching entry older than the current
 /// history cursor; a miss never modifies the editor or saved draft.
 /// The original draft remains recoverable via history_next().
 pub fn history_search_previous(&mut self,prefix:&[u8])
  ->Result<usize,Error>{
  if prefix.is_empty()||prefix.len()>MAX_LINE||
     prefix.iter().any(|&b|!(b' '..=b'~').contains(&b)){
   return Err(Error::Invalid);
  }
  let end=self.history_cursor.unwrap_or(self.history_len);
  let index=(0..end).rev().find(|&i|
   self.history[i].is_some_and(|entry|
      entry.bytes[..entry.len].starts_with(prefix))).ok_or(Error::Invalid)?;
  let selected=self.history[index].ok_or(Error::Invalid)?;
  if self.history_cursor.is_none(){
   self.history_draft.fill(0);
   self.history_draft_len=self.length();
   self.history_draft[..self.history_draft_len]
    .copy_from_slice(self.editor.bytes());
  }
  self.editor.replace_line(&selected.bytes[..selected.len])?;
  self.history_cursor=Some(index);
  Ok(selected.len)
 }
 pub fn history_next(&mut self)->Result<usize,Error>{
  let index=self.history_cursor.ok_or(Error::Invalid)?;
  if index+1==self.history_len{
   let n=self.history_draft_len;
   self.editor.replace_line(&self.history_draft[..n])?;
   self.history_cursor=None;self.history_draft.fill(0);
   self.history_draft_len=0;return Ok(n);
  }
  let next=index+1;
  let entry=self.history[next].ok_or(Error::Invalid)?;
  self.editor.replace_line(&entry.bytes[..entry.len])?;
  self.history_cursor=Some(next);
  Ok(entry.len)
 }
 pub fn feed(&mut self,key:u8,out:&mut[u8])->Result<Event,Error>{
  match key{
   b'\n'|b'\r'=>{
    let event=self.editor.edit(EditKey::Submit,out)?;
    if let Event::Submitted(n)=event{
     self.remember(&out[..n]);
     self.history_cursor=None;
     self.history_draft.fill(0);self.history_draft_len=0;
    }
    Ok(event)
   },
   8|127=>{
    let event=self.editor.edit(EditKey::Backspace,&mut[])?;
    self.history_cursor=None;Ok(event)
   },
   b' '..=b'~'=>{
    let event=self.editor.edit(EditKey::Insert(key),&mut[])?;
    self.history_cursor=None;Ok(event)
   },
   _=>Err(Error::Invalid)
  }
 }
}
/// Orbit keyboard task E1-E5: cursor-aware editing, independent of PS/2 scan
/// codes or privileged IPC. Only validated ASCII is currently supported.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum EditKey{Left,Right,Home,End,Insert(u8),Backspace,Delete,Submit,Clear}
/// Bounded line editor; not wired to GUI Terminal until the live input broker
/// can deliver stable keyboard event codes. No allocation or raw user pointers.
pub struct LineEditor{
    line:[u8;MAX_LINE],
    len:usize,
    caret:usize
}
impl LineEditor{
    pub const fn new()->Self{Self{line:[0;MAX_LINE],len:0,caret:0}}
    pub fn caret(&self)->usize{self.caret}
    pub fn bytes(&self)->&[u8]{&self.line[..self.len]}
    pub fn clear(&mut self){self.line.fill(0);self.len=0;self.caret=0;}
    /// Replace all edited text in one bounded transaction, preserving
    /// old content and caret on invalid input/overflow.
    pub fn replace_line(&mut self,next:&[u8])->Result<(),Error>{
        if next.len()>MAX_LINE{return Err(Error::Full);}
        if next.iter().any(|&c|!(b' '..=b'~').contains(&c)){
            return Err(Error::Invalid);
        }
        self.clear();self.line[..next.len()].copy_from_slice(next);
        self.len=next.len();self.caret=self.len;Ok(())
    }
    /// Paste multiple ASCII characters atomically into the current caret.
    /// Source may contain no newlines, controls or NUL bytes.
    pub fn paste(&mut self,source:&[u8])->Result<(),Error>{
        let available=MAX_LINE-self.len;
        if source.len()>available{return Err(Error::Full);}
        if source.iter().any(|&b|!(b' '..=b'~').contains(&b)){
            return Err(Error::Invalid);
        }
        self.line.copy_within(self.caret..self.len,self.caret+source.len());
        self.line[self.caret..self.caret+source.len()].copy_from_slice(source);
        self.len+=source.len();self.caret+=source.len();Ok(())
    }
    /// Navigate ASCII words without assuming the cursor points to a glyph.
    pub fn previous_word(&mut self)->usize{
        while self.caret>0&&self.line[self.caret-1]==b' '{self.caret-=1;}
        while self.caret>0&&self.line[self.caret-1]!=b' '{self.caret-=1;}
        self.caret
    }
    pub fn next_word(&mut self)->usize{
        while self.caret<self.len&&self.line[self.caret]!=b' '{self.caret+=1;}
        while self.caret<self.len&&self.line[self.caret]==b' '{self.caret+=1;}
        self.caret
    }
    /// Delete the word immediately before the caret, including separation,
    /// and scrub vacated tail bytes.
    pub fn erase_previous_word(&mut self)->Event{
        let end=self.caret;let start=self.previous_word();
        if end==start{return Event::Empty;}
        self.line.copy_within(end..self.len,start);
        self.len-=end-start;self.line[self.len..].fill(0);
        Event::Erased
    }
    /// Replace one source range as a single bounded atomic edit, always
    /// leaving the cursor immediately after the inserted replacement.
    pub fn replace_range(&mut self,start:usize,end:usize,next:&[u8])
        ->Result<Event,Error>{
        if start>end||end>self.len{return Err(Error::Invalid);}
        if next.iter().any(|&byte|!(b' '..=b'~').contains(&byte)){
            return Err(Error::Invalid);
        }
        let new_len=self.len-(end-start);
        let new_len=new_len.checked_add(next.len()).ok_or(Error::Full)?;
        if new_len>MAX_LINE{return Err(Error::Full);}
        if start==end&&next.is_empty(){return Ok(Event::Empty);}
        let mut new=[0u8;MAX_LINE];
        new[..start].copy_from_slice(&self.line[..start]);
        new[start..start+next.len()].copy_from_slice(next);
        new[start+next.len()..new_len].copy_from_slice(&self.line[end..self.len]);
        self.line=new;self.len=new_len;self.caret=start+next.len();
        Ok(if next.is_empty(){Event::Erased}else{Event::Typed})
    }
    /// Replace every non-overlapping literal ASCII occurrence in one bounded
    /// edit. Errors never alter source bytes or the cursor. A successful
    /// edit places the caret after the final resulting byte.
    pub fn replace_all_literal(&mut self,needle:&[u8],replacement:&[u8])
        ->Result<usize,Error>{
        if needle.is_empty()||needle.len()>MAX_LINE||
           needle.iter().chain(replacement.iter()).any(|b|
               !(b' '..=b'~').contains(b)){
            return Err(Error::Invalid);
        }
        let mut staged=[0u8;MAX_LINE];
        let mut source=0usize;
        let mut used=0usize;
        let mut changed=0usize;
        while source<self.len {
            if self.line[source..self.len].starts_with(needle){
                let end=used.checked_add(replacement.len()).ok_or(Error::Full)?;
                if end>MAX_LINE{return Err(Error::Full);}
                staged[used..end].copy_from_slice(replacement);
                used=end;source+=needle.len();changed+=1;
            }else{
                if used==MAX_LINE{return Err(Error::Full);}
                staged[used]=self.line[source];used+=1;source+=1;
            }
        }
        if changed>0{
            self.line=staged;self.len=used;self.caret=used;
        }
        Ok(changed)
    }
    /// Copy a selection only after destination preflight.
    pub fn copy_range(&self,start:usize,end:usize,out:&mut[u8])
        ->Result<usize,Error>{
        if start>end||end>self.len{return Err(Error::Invalid);}
        let size=end-start;
        if out.len()<size{return Err(Error::TooSmall);}
        out[..size].copy_from_slice(&self.line[start..end]);
        Ok(size)
    }
    pub fn edit(&mut self,key:EditKey,out:&mut[u8])->Result<Event,Error>{
        match key{
            // E1: move cursor without causing side effects or underflow.
            EditKey::Left=>{self.caret=self.caret.saturating_sub(1);Ok(Event::Empty)}
            EditKey::Right=>{
                self.caret=(self.caret+1).min(self.len);Ok(Event::Empty)
            }
            // E2: jump to either end, bounded by actual line length.
            EditKey::Home=>{self.caret=0;Ok(Event::Empty)}
            EditKey::End=>{self.caret=self.len;Ok(Event::Empty)}
            // E3: insert within the buffer, shifting only occupied bytes.
            EditKey::Insert(ch)=>{
                if !(b' '..=b'~').contains(&ch){return Err(Error::Invalid);}
                if self.len==MAX_LINE{return Err(Error::Full);}
                for i in (self.caret..self.len).rev(){
                    self.line[i+1]=self.line[i];
                }
                self.line[self.caret]=ch;self.len+=1;self.caret+=1;
                Ok(Event::Typed)
            }
            // E4: delete/backspace with non-leaking tail zeroing.
            EditKey::Delete|EditKey::Backspace=>{
                let index=if key==EditKey::Backspace{
                    if self.caret==0{return Ok(Event::Empty);}
                    self.caret-1
                }else{
                    if self.caret==self.len{return Ok(Event::Empty);}
                    self.caret
                };
                for i in index..self.len-1{self.line[i]=self.line[i+1];}
                self.len-=1;self.line[self.len]=0;
                if key==EditKey::Backspace{self.caret-=1;}
                Ok(Event::Erased)
            }
            EditKey::Clear=>{
                self.clear();Ok(Event::Erased)
            }
            // E5: failed submit must preserve entire state (no partial line).
            EditKey::Submit=>{
                if self.len==0{return Ok(Event::Empty);}
                if out.len()<self.len{return Err(Error::TooSmall);}
                let size=self.len;out[..size].copy_from_slice(self.bytes());
                self.clear();Ok(Event::Submitted(size))
            }
        }
    }
}
#[cfg(test)] mod tests{
 use super::*;
 #[test] fn real_typing_backspace_and_return(){
  let mut input=InputLine::new();let mut buf=[0u8;MAX_LINE];
  for &b in b"write /note helloo"{
   assert_eq!(input.feed(b,&mut buf),Ok(Event::Typed));
  }
  assert_eq!(input.feed(8,&mut buf),Ok(Event::Erased));
  assert_eq!(input.feed(b'\r',&mut buf),Ok(Event::Submitted(17)));
  assert_eq!(&buf[..17],b"write /note hello");
  assert_eq!(input.length(),0);
 }
 #[test] fn input_overflow_and_failed_submission_never_drop_data(){
  let mut input=InputLine::new();
  for _ in 0..MAX_LINE{input.feed(b'x',&mut[]).unwrap();}
  assert_eq!(input.feed(b'a',&mut[]),Err(Error::Full));
  assert_eq!(input.feed(0,&mut[]),Err(Error::Invalid));
  assert_eq!(input.feed(b'\n',&mut[]),Err(Error::TooSmall));
  assert_eq!(input.length(),MAX_LINE);
  let mut out=[0u8;MAX_LINE];
  assert_eq!(input.feed(b'\n',&mut out),Ok(Event::Submitted(MAX_LINE)));
  assert_eq!(out,[b'x';MAX_LINE]);
 }
 #[test] fn empty_command_backspace_does_not_underflow(){
  let mut input=InputLine::new();
  assert_eq!(input.feed(8,&mut[]),Ok(Event::Empty));
  assert_eq!(input.feed(b'\n',&mut[]),Ok(Event::Empty));
 }

 #[test]fn orbit_editor_caret_navigation_and_home_end(){
  let mut e=LineEditor::new();
  for &b in b"abc"{e.edit(EditKey::Insert(b),&mut[]).unwrap();}
  e.edit(EditKey::Left,&mut[]).unwrap();
  e.edit(EditKey::Left,&mut[]).unwrap();
  assert_eq!(e.caret(),1);
  e.edit(EditKey::Home,&mut[]).unwrap();
  for _ in 0..4{e.edit(EditKey::Left,&mut[]).unwrap();}
  assert_eq!(e.caret(),0);
  e.edit(EditKey::End,&mut[]).unwrap();
  for _ in 0..4{e.edit(EditKey::Right,&mut[]).unwrap();}
  assert_eq!(e.caret(),3);
 }
 #[test]fn orbit_editor_middle_insertion_delete_and_backspace(){
  let mut e=LineEditor::new();
  for &b in b"ac"{e.edit(EditKey::Insert(b),&mut[]).unwrap();}
  e.edit(EditKey::Left,&mut[]).unwrap();
  e.edit(EditKey::Insert(b'b'),&mut[]).unwrap();
  assert_eq!(e.bytes(),b"abc");
  e.edit(EditKey::Backspace,&mut[]).unwrap();
  assert_eq!(e.bytes(),b"ac");
  e.edit(EditKey::Delete,&mut[]).unwrap();
  assert_eq!(e.bytes(),b"a");
  assert_eq!(e.edit(EditKey::Delete,&mut[]),Ok(Event::Empty));
 }
 #[test]fn orbit_editor_submit_atomic_and_clear(){
  let mut e=LineEditor::new();let mut out=[0xaa;8];
  for &b in b"hello"{e.edit(EditKey::Insert(b),&mut[]).unwrap();}
  assert_eq!(e.edit(EditKey::Submit,&mut out[..3]),Err(Error::TooSmall));
  assert_eq!(e.bytes(),b"hello");assert_eq!(e.caret(),5);
  assert_eq!(out,[0xaa;8]);
  assert_eq!(e.edit(EditKey::Submit,&mut out),Ok(Event::Submitted(5)));
  assert_eq!(&out[..5],b"hello");
  assert_eq!(e.bytes(),b"");assert_eq!(e.caret(),0);
  e.edit(EditKey::Insert(b'a'),&mut[]).unwrap();
  e.edit(EditKey::Clear,&mut[]).unwrap();
  assert!(e.bytes().is_empty());
 }
 #[test]fn orbit_editor_capacity_and_invalid_input_never_mutate(){
  let mut e=LineEditor::new();
  for _ in 0..MAX_LINE{e.edit(EditKey::Insert(b'x'),&mut[]).unwrap();}
  assert_eq!(e.edit(EditKey::Insert(b'y'),&mut[]),Err(Error::Full));
  assert_eq!(e.edit(EditKey::Insert(0),&mut[]),Err(Error::Invalid));
  assert_eq!(e.bytes().len(),MAX_LINE);assert_eq!(e.caret(),MAX_LINE);
 }

 #[test]fn orbit_large_editor_paste_replace_and_selection(){
  let mut e=LineEditor::new();
  e.replace_line(b"deploy service").unwrap();
  e.edit(EditKey::Home,&mut[]).unwrap();
  e.paste(b"safe ").unwrap();
  assert_eq!(e.bytes(),b"safe deploy service");
  let mut buf=[0u8;7];
  assert_eq!(e.copy_range(5,11,&mut buf),Ok(6));
  assert_eq!(&buf[..6],b"deploy");
  assert_eq!(e.copy_range(0,12,&mut buf),Err(Error::TooSmall));
  assert_eq!(e.replace_line(b"bad\nline"),Err(Error::Invalid));
  assert_eq!(e.bytes(),b"safe deploy service");
 }
 #[test]fn orbit_editor_word_nav_and_delete_multiple_words(){
  let mut e=LineEditor::new();e.replace_line(b"one two   three").unwrap();
  assert_eq!(e.previous_word(),10);
  assert_eq!(e.erase_previous_word(),Event::Erased);
  assert_eq!(e.bytes(),b"one three");
  e.edit(EditKey::End,&mut[]).unwrap();
  assert_eq!(e.previous_word(),4);
  e.edit(EditKey::Home,&mut[]).unwrap();
  assert_eq!(e.next_word(),4);
 }
 #[test]fn orbit_editor_full_buffer_paste_is_fail_closed(){
  let mut e=LineEditor::new();e.replace_line(&[b'x';MAX_LINE-1]).unwrap();
  let original=e.caret();
  assert_eq!(e.paste(b"ab"),Err(Error::Full));
  assert_eq!(e.paste(b"\n"),Err(Error::Invalid));
  assert_eq!(e.caret(),original);assert_eq!(e.bytes().len(),MAX_LINE-1);
  assert_eq!(e.paste(b"z"),Ok(()));
  assert_eq!(e.bytes().len(),MAX_LINE);
 }

 #[test]fn r044_live_line_editor_cursor_insertion_and_forward_delete(){
  let mut input=InputLine::new();
  input.set_line(b"wite /x hi").unwrap();
  input.action(EditKey::Home).unwrap();
  input.action(EditKey::Right).unwrap();
  input.feed(b'r',&mut[]).unwrap();
  assert_eq!(input.bytes(),b"write /x hi");
  assert_eq!(input.caret(),2);
  input.action(EditKey::Right).unwrap();
  input.action(EditKey::Delete).unwrap();
  assert_eq!(input.bytes(),b"wrie /x hi");
 }
 #[test]fn r044_input_caret_navigation_and_word_delete_are_bounded(){
  let mut x=InputLine::new();x.set_line(b"one two three").unwrap();
  x.action(EditKey::Home).unwrap();
  x.action(EditKey::Left).unwrap();assert_eq!(x.caret(),0);
  x.word_right();assert_eq!(x.caret(),4);
  x.word_right();assert_eq!(x.caret(),8);
  x.erase_word();assert_eq!(x.bytes(),b"one three");
  assert_eq!(x.caret(),4);
  x.action(EditKey::End).unwrap();assert_eq!(x.caret(),9);
  x.word_left();assert_eq!(x.caret(),4);
  x.delete_to_end();assert_eq!(x.bytes(),b"one ");
  assert_eq!(x.action(EditKey::Submit),Err(Error::Invalid));
 }
 #[test]fn r044_selection_cut_paste_and_cursor_are_owner_memory_bounded(){
  let mut x=InputLine::new();x.set_line(b"one two three").unwrap();
  assert_eq!(x.copy_range(4,7),Ok(3));
  assert_eq!(x.clipboard_bytes(),b"two");
  assert_eq!(x.cut_range(4,8),Ok(4));
  assert_eq!(x.bytes(),b"one three");
  assert_eq!(x.clipboard_bytes(),b"two ");
  x.action(EditKey::End).unwrap();
  x.paste_clipboard().unwrap();
  assert_eq!(x.bytes(),b"one threetwo ");
  x.reset();assert!(x.is_empty());assert_eq!(x.clipboard_bytes(),b"two ");
  x.forget();assert!(x.clipboard_bytes().is_empty());
 }
 #[test]fn r044_selection_and_paste_fail_without_partial_buffer_modification(){
  let mut x=InputLine::new();x.set_line(b"abc").unwrap();
  x.copy_range(0,2).unwrap();
  let original=*b"abc";
  assert_eq!(x.copy_range(0,99),Err(Error::Invalid));
  assert_eq!(x.clipboard_bytes(),b"ab");
  assert_eq!(x.replace_range(0,1,b"bad\nline"),Err(Error::Invalid));
  assert_eq!(x.bytes(),original);
  x.set_line(&[b'x';MAX_LINE]).unwrap();
  assert_eq!(x.paste_clipboard(),Err(Error::Full));
  assert_eq!(x.bytes(),&[b'x';MAX_LINE]);
 }
 #[test]fn r044_replacement_overlapping_selection_preserves_cursor_and_tail(){
  let mut input=InputLine::new();input.set_line(b"abc-def").unwrap();
  assert_eq!(input.replace_range(2,5,b"XYZ"),Ok(Event::Typed));
  assert_eq!(input.bytes(),b"abXYZef");
  assert_eq!(input.caret(),5);
  assert_eq!(input.replace_range(2,5,b""),Ok(Event::Erased));
  assert_eq!(input.bytes(),b"abef");
  assert_eq!(input.caret(),2);
 }
 #[test]fn r057_editor_replace_all_literal_and_cursor_postcondition(){
  let mut e=LineEditor::new();e.replace_line(b"foo + foo + foo").unwrap();
  e.edit(EditKey::Home,&mut[]).unwrap();
  assert_eq!(e.replace_all_literal(b"foo",b"barz"),Ok(3));
  assert_eq!(e.bytes(),b"barz + barz + barz");
  assert_eq!(e.caret(),e.bytes().len());
  let old=e.caret();
  assert_eq!(e.replace_all_literal(b"missing",b"x"),Ok(0));
  assert_eq!(e.caret(),old);
  assert_eq!(e.replace_all_literal(b"barz",b""),Ok(3));
  assert_eq!(e.bytes(),b" +  + ");
 }
 #[test]fn r057_editor_replace_is_atomic_for_capacity_and_invalid_input(){
  let mut e=LineEditor::new();
  e.replace_line(&[b'a';MAX_LINE-1]).unwrap();
  let cursor=e.caret();
  assert_eq!(e.replace_all_literal(b"a",b"aa"),Err(Error::Full));
  assert_eq!(e.replace_all_literal(b"",b"x"),Err(Error::Invalid));
  assert_eq!(e.replace_all_literal(b"a",b"bad\n"),Err(Error::Invalid));
  assert_eq!(e.bytes(),&[b'a';MAX_LINE-1]);
  assert_eq!(e.caret(),cursor);
 }
 #[test]fn r057_input_line_replace_all_preserves_clipboard(){
  let mut line=InputLine::new();line.set_line(b"cat cat and cat").unwrap();
  line.copy_range(0,3).unwrap();
  assert_eq!(line.replace_all_literal(b"cat",b"dog"),Ok(3));
  assert_eq!(line.bytes(),b"dog dog and dog");
  assert_eq!(line.clipboard_bytes(),b"cat");
  assert_eq!(line.replace_all_literal(b"notfound",b"z"),Ok(0));
  assert_eq!(line.bytes(),b"dog dog and dog");
 }
 #[test]fn r058_input_history_navigation_preserves_unsubmitted_draft(){
  let mut line=InputLine::new();let mut out=[0u8;MAX_LINE];
  for cmd in [b"pwd".as_slice(),b"ls",b"cat /a"]{
   line.set_line(cmd).unwrap();
   assert_eq!(line.feed(b'\n',&mut out),Ok(Event::Submitted(cmd.len())));
  }
  assert_eq!(line.history_count(),3);
  line.set_line(b"new draft").unwrap();
  assert_eq!(line.history_previous(),Ok(6));
  assert_eq!(line.bytes(),b"cat /a");
  assert_eq!(line.history_previous(),Ok(2));
  assert_eq!(line.bytes(),b"ls");
  assert_eq!(line.history_previous(),Ok(3));
  assert_eq!(line.bytes(),b"pwd");
  assert_eq!(line.history_previous(),Ok(3));
  assert_eq!(line.history_next(),Ok(2));
  assert_eq!(line.history_next(),Ok(6));
  assert_eq!(line.history_next(),Ok(9));
  assert_eq!(line.bytes(),b"new draft");
  assert_eq!(line.history_next(),Err(Error::Invalid));
 }
 #[test]fn r058_history_excludes_failed_submit_and_dedupes_recents(){
  let mut line=InputLine::new();let mut small=[0xaa;2];
  line.set_line(b"long").unwrap();
  assert_eq!(line.feed(b'\n',&mut small),Err(Error::TooSmall));
  assert_eq!(line.history_count(),0);
  assert_eq!(line.bytes(),b"long");
  let mut out=[0u8;MAX_LINE];
  assert_eq!(line.feed(b'\n',&mut out),Ok(Event::Submitted(4)));
  line.set_line(b"long").unwrap();
  line.feed(b'\n',&mut out).unwrap();
  assert_eq!(line.history_count(),1);
  for word in [b"one".as_slice(),b"two",b"three",b"four"]{
   line.set_line(word).unwrap();line.feed(b'\n',&mut out).unwrap();
  }
  assert_eq!(line.history_count(),HISTORY_LIMIT);
  assert_eq!(line.history_previous(),Ok(4));
  assert_eq!(line.bytes(),b"four");
  line.history_previous().unwrap();line.history_previous().unwrap();
  assert_eq!(line.history_previous(),Ok(3));
  assert_eq!(line.bytes(),b"one");
  line.forget();
  assert_eq!(line.history_count(),0);
  assert_eq!(line.history_previous(),Err(Error::Invalid));
 }
 #[test]fn r059_reverse_history_prefix_search_restores_original_draft(){
  let mut input=InputLine::new();let mut out=[0u8;MAX_LINE];
  for command in [b"build one".as_slice(),b"help",b"build two",
                  b"build three",b"status"]{
   input.set_line(command).unwrap();
   input.feed(b'\n',&mut out).unwrap();
  }
  input.set_line(b"unfinished").unwrap();
  assert_eq!(input.history_search_previous(b"build"),Ok(11));
  assert_eq!(input.bytes(),b"build three");
  assert_eq!(input.history_search_previous(b"build"),Ok(9));
  assert_eq!(input.bytes(),b"build two");
  assert_eq!(input.history_search_previous(b"build"),Err(Error::Invalid));
  assert_eq!(input.bytes(),b"build two");
  // The search does not lose the user's original in-progress draft.
  assert_eq!(input.history_next(),Ok(11));
  assert_eq!(input.bytes(),b"build three");
  assert_eq!(input.history_next(),Ok(6));
  assert_eq!(input.bytes(),b"status");
  assert_eq!(input.history_next(),Ok(10));
  assert_eq!(input.bytes(),b"unfinished");
 }
 #[test]fn r059_reverse_search_miss_and_invalid_prefix_leave_draft_intact(){
  let mut input=InputLine::new();let mut out=[0u8;MAX_LINE];
  input.set_line(b"cat /notes").unwrap();input.feed(b'\n',&mut out).unwrap();
  input.set_line(b"work in progress").unwrap();
  for prefix in [b"".as_slice(),b"wrong",b"cat\n",b"CAT"]{
   assert_eq!(input.history_search_previous(prefix),Err(Error::Invalid));
   assert_eq!(input.bytes(),b"work in progress");
  }
  assert_eq!(input.history_search_previous(b"cat"),Ok(10));
  assert_eq!(input.bytes(),b"cat /notes");
  input.forget();
  assert_eq!(input.history_search_previous(b"cat"),Err(Error::Invalid));
  assert_eq!(input.history_count(),0);
 }
 #[test]fn r042_kill_and_safe_restore_preserve_draft_invariants(){
  let mut line=InputLine::new();
  line.set_line(b"echo existing").unwrap();
  assert_eq!(line.restore_if_empty(b"other"),Err(Error::Invalid));
  assert_eq!(line.bytes(),b"echo existing");
  assert_eq!(line.kill_line(),Event::Erased);
  assert_eq!(line.kill_line(),Event::Empty);
  line.restore_if_empty(b"pwd").unwrap();
  assert_eq!(line.bytes(),b"pwd");
 }
 #[test]fn r041_terminal_ctrl_w_and_ctrl_u_are_bounded(){
  let mut line=InputLine::new();line.set_line(b"echo hello world").unwrap();
  assert_eq!(line.erase_word(),Event::Erased);
  assert_eq!(line.bytes(),b"echo hello ");
  assert_eq!(line.erase_word(),Event::Erased);
  assert_eq!(line.bytes(),b"echo ");
  line.clear_line();assert!(line.bytes().is_empty());
  assert_eq!(line.erase_word(),Event::Empty);
 }
 #[test]fn r040_terminal_history_restoration_is_atomic_and_bounded(){
  let mut line=InputLine::new();
  line.set_line(b"write /home/a persisted").unwrap();
  assert_eq!(line.bytes(),b"write /home/a persisted");
  assert_eq!(line.set_line(b"unsafe\ncommand"),Err(Error::Invalid));
  assert_eq!(line.bytes(),b"write /home/a persisted");
  assert_eq!(line.set_line(&[b'x';MAX_LINE+1]),Err(Error::Full));
  assert_eq!(line.bytes(),b"write /home/a persisted");
  let mut output=[0u8;MAX_LINE];
  assert_eq!(line.feed(b'\n',&mut output),Ok(Event::Submitted(23)));
  assert_eq!(&output[..23],b"write /home/a persisted");
  assert!(line.bytes().is_empty());
 }

}

/// R060: forward prefix search complements reverse history search. Only
/// successfully submitted entries are eligible; a missing match leaves
/// both the selected command and the original unsent draft intact.
impl InputLine{
 pub fn history_search_next(&mut self,prefix:&[u8])->Result<usize,Error>{
  if prefix.is_empty()||prefix.len()>MAX_LINE||
     prefix.iter().any(|&b|!(b' '..=b'~').contains(&b)){
   return Err(Error::Invalid);
  }
  let current=self.history_cursor.ok_or(Error::Invalid)?;
  let index=((current+1)..self.history_len).find(|&i|
   self.history[i].is_some_and(|entry|
     entry.bytes[..entry.len].starts_with(prefix)))
   .ok_or(Error::Invalid)?;
  let selected=self.history[index].ok_or(Error::Invalid)?;
  self.editor.replace_line(&selected.bytes[..selected.len])?;
  self.history_cursor=Some(index);
  Ok(selected.len)
 }
}
#[cfg(test)]
mod r060_forward_history_tests{
 use super::*;
 fn populated()->InputLine{
  let mut input=InputLine::new();
  let mut out=[0u8;MAX_LINE];
  for line in [b"git status".as_slice(),b"cargo test",
               b"git diff",b"git push"]{
   input.set_line(line).unwrap();
   assert_eq!(input.feed(b'\n',&mut out),Ok(Event::Submitted(line.len())));
  }
  input.set_line(b"unsent draft").unwrap();
  input
 }
 #[test]fn forward_matches_only_newer_history_and_can_restore_draft(){
  let mut input=populated();
  assert_eq!(input.history_search_previous(b"git"),Ok(8));
  assert_eq!(input.bytes(),b"git push");
  assert_eq!(input.history_search_previous(b"git"),Ok(8));
  assert_eq!(input.bytes(),b"git diff");
  assert_eq!(input.history_search_previous(b"git"),Ok(10));
  assert_eq!(input.bytes(),b"git status");
  assert_eq!(input.history_search_next(b"git"),Ok(8));
  assert_eq!(input.bytes(),b"git diff");
  assert_eq!(input.history_search_next(b"git"),Ok(8));
  assert_eq!(input.bytes(),b"git push");
  assert_eq!(input.history_next(),Ok(12));
  assert_eq!(input.bytes(),b"unsent draft");
 }
 #[test]fn bad_prefix_absent_match_and_unsaved_line_are_fail_closed(){
  let mut input=populated();
  assert_eq!(input.history_search_next(b"git"),Err(Error::Invalid));
  assert_eq!(input.bytes(),b"unsent draft");
  input.history_search_previous(b"git").unwrap();
  let saved=b"git push";
  assert_eq!(input.history_search_next(b"missing"),Err(Error::Invalid));
  assert_eq!(input.history_search_next(b"\n"),Err(Error::Invalid));
  assert_eq!(input.bytes(),saved);
  assert_eq!(input.history_next(),Ok(12));
  assert_eq!(input.bytes(),b"unsent draft");
 }
}


/// R061: interactive terminal input supports reverse substring search in
/// the fixed-size successful-command ring, not only prefix matching.
/// A failed search does not touch an unsent draft or selection state.
impl InputLine{
 pub fn history_search_contains(&mut self,needle:&[u8])
  ->Result<usize,Error>{
  if needle.is_empty()||needle.len()>MAX_LINE||
     needle.iter().any(|&b|!(b' '..=b'~').contains(&b)){
    return Err(Error::Invalid);
  }
  let end=self.history_cursor.unwrap_or(self.history_len);
  let index=(0..end).rev().find(|&i|self.history[i].is_some_and(|entry|
    entry.bytes[..entry.len].windows(needle.len()).any(|part|part==needle)))
    .ok_or(Error::Invalid)?;
  let selected=self.history[index].ok_or(Error::Invalid)?;
  if self.history_cursor.is_none(){
   self.history_draft.fill(0);
   self.history_draft_len=self.length();
   self.history_draft[..self.history_draft_len]
      .copy_from_slice(self.editor.bytes());
  }
  self.editor.replace_line(&selected.bytes[..selected.len])?;
  self.history_cursor=Some(index);
  Ok(selected.len)
 }
 pub fn transpose_near_caret(&mut self)->Result<(),Error>{
  self.editor.transpose_adjacent()?;
  self.history_cursor=None;
  self.history_draft.fill(0);self.history_draft_len=0;
  Ok(())
 }
}
/// Swap two adjacent ASCII characters around the live caret without
/// changing the line length or creating a partial edit. With the caret
/// at the end, transpose the final two characters (typical shell usage).
impl LineEditor{
 pub fn transpose_adjacent(&mut self)->Result<(),Error>{
  if self.len<2||self.caret==0{return Err(Error::Invalid);}
  let index=if self.caret==self.len{
   self.len-2
  }else{
   self.caret-1
  };
  self.line.swap(index,index+1);
  self.caret=(index+2).min(self.len);
  Ok(())
 }
}
#[cfg(test)]
mod r061_terminal_interaction_tests{
 use super::*;
 fn ready()->InputLine{
  let mut input=InputLine::new();
  let mut out=[0u8;MAX_LINE];
  for cmd in [b"git status".as_slice(),b"cargo test",b"git diff",b"git push"]{
   input.set_line(cmd).unwrap();
   assert_eq!(input.feed(b'\n',&mut out),Ok(Event::Submitted(cmd.len())));
  }
  input.set_line(b"unsaved").unwrap();
  input
 }
 #[test]fn reverse_substring_search_moves_across_history_and_restores_draft(){
  let mut input=ready();
  assert_eq!(input.history_search_contains(b"git"),Ok(8));
  assert_eq!(input.bytes(),b"git push");
  assert_eq!(input.history_search_contains(b"it"),Ok(8));
  assert_eq!(input.bytes(),b"git diff");
  assert_eq!(input.history_search_contains(b"status"),Ok(10));
  assert_eq!(input.bytes(),b"git status");
  assert_eq!(input.history_next(),Ok(10));
  assert_eq!(input.history_next(),Ok(8));
  assert_eq!(input.history_next(),Ok(8));
  assert_eq!(input.history_next(),Ok(7));
  assert_eq!(input.bytes(),b"unsaved");
 }
 #[test]fn bad_or_absent_substrings_do_not_modify_history_or_draft(){
  let mut input=ready();
  assert_eq!(input.history_search_contains(b""),Err(Error::Invalid));
  assert_eq!(input.history_search_contains(b"\n"),Err(Error::Invalid));
  assert_eq!(input.history_search_contains(b"missing"),Err(Error::Invalid));
  assert_eq!(input.bytes(),b"unsaved");
  input.history_search_contains(b"git").unwrap();
  assert_eq!(input.history_search_contains(b"absent"),Err(Error::Invalid));
  assert_eq!(input.bytes(),b"git push");
  assert_eq!(input.history_next(),Ok(7));
  assert_eq!(input.bytes(),b"unsaved");
 }
 #[test]fn transpose_at_caret_and_end_produces_real_editable_command(){
  let mut line=LineEditor::new();
  line.replace_line(b"acb").unwrap();
  line.edit(EditKey::Left,&mut[]).unwrap();
  assert_eq!(line.caret(),2);
  line.transpose_adjacent().unwrap();
  assert_eq!(line.bytes(),b"abc");
  assert_eq!(line.caret(),3);
  line.transpose_adjacent().unwrap();
  assert_eq!(line.bytes(),b"acb");
  let mut input=ready();
  input.set_line(b"echo hlelo").unwrap();
  input.transpose_near_caret().unwrap();
  assert_eq!(input.bytes(),b"echo hleol");
 }
 #[test]fn zero_caret_and_too_short_input_never_partially_swap(){
  let mut line=LineEditor::new();
  line.replace_line(b"a").unwrap();
  assert_eq!(line.transpose_adjacent(),Err(Error::Invalid));
  line.replace_line(b"abcd").unwrap();
  line.edit(EditKey::Home,&mut[]).unwrap();
  assert_eq!(line.transpose_adjacent(),Err(Error::Invalid));
  assert_eq!(line.caret(),0);
  assert_eq!(line.bytes(),b"abcd");
 }
}


/// R062: a complete keyboard edit macro is published or rejected as ONE
/// change to the authenticated Terminal input line. No Submit key may be
/// smuggled into a macro and no invalid trailing key may partially modify
/// the live buffer. This deliberately does not touch the OS command runner.
impl InputLine{
 pub fn edit_batch(&mut self,keys:&[EditKey])->Result<usize,Error>{
  if keys.is_empty()||keys.len()>64||keys.contains(&EditKey::Submit){
   return Err(Error::Invalid);
  }
  let mut staged=LineEditor{
   line:self.editor.line,len:self.editor.len,caret:self.editor.caret
  };
  for key in keys{
   staged.edit(*key,&mut[])?;
  }
  let changed=staged.bytes()!=self.editor.bytes();
  self.editor=staged;
  if changed{
   self.history_cursor=None;
   self.history_draft.fill(0);self.history_draft_len=0;
  }
  Ok(keys.len())
 }
}
#[cfg(test)]
mod r062_atomic_keyboard_batch_tests{
 use super::*;
 #[test]fn typed_navigation_and_insertion_form_one_edit(){
  let mut input=InputLine::new();
  input.set_line(b"git stat").unwrap();
  assert_eq!(input.edit_batch(&[
   EditKey::End,EditKey::Insert(b'u'),EditKey::Insert(b's'),
   EditKey::Home,EditKey::Right,EditKey::Right,EditKey::Right,
   EditKey::Insert(b' ')
  ]),Ok(8));
  assert_eq!(input.bytes(),b"git  status");
  assert_eq!(input.caret(),4);
 }
 #[test]fn late_invalid_character_and_overflow_leave_live_draft_intact(){
  let mut input=InputLine::new();
  input.set_line(b"safe").unwrap();
  let before=input.caret();
  assert_eq!(input.edit_batch(&[
    EditKey::Insert(b'!'),EditKey::Insert(0)
  ]),Err(Error::Invalid));
  assert_eq!(input.bytes(),b"safe");
  assert_eq!(input.caret(),before);
  input.set_line(&[b'x';MAX_LINE]).unwrap();
  assert_eq!(input.edit_batch(&[
    EditKey::Left,EditKey::Insert(b'!'),
  ]),Err(Error::Full));
  assert_eq!(input.bytes(),&[b'x';MAX_LINE]);
  assert_eq!(input.caret(),MAX_LINE);
 }
 #[test]fn submit_in_macro_and_empty_macro_do_not_execute_or_clear_command(){
  let mut input=InputLine::new();
  input.set_line(b"write /note safe").unwrap();
  assert_eq!(input.edit_batch(&[
    EditKey::Insert(b'!'),EditKey::Submit
  ]),Err(Error::Invalid));
  assert_eq!(input.edit_batch(&[]),Err(Error::Invalid));
  assert_eq!(input.bytes(),b"write /note safe");
  assert_eq!(input.history_count(),0);
 }
}


/// R063: complete a live Terminal draft only when exactly one previously
/// submitted command matches its ASCII prefix. No command is executed; the
/// keyboard broker still requires a separate explicit Enter event.
/// Ambiguity, stale history, empty/nonprintable prefixes leave text intact.
impl InputLine{
 pub fn complete_from_history(&mut self)->Result<usize,Error>{
  let prefix=self.editor.bytes();
  if prefix.is_empty()||prefix.iter().any(|&b|!(b' '..=b'~').contains(&b)){
   return Err(Error::Invalid);
  }
  let mut selected=None;
  for entry in self.history.iter().flatten(){
   if entry.bytes[..entry.len].starts_with(prefix){
    if selected.is_some(){return Err(Error::Invalid);}
    selected=Some(*entry);
   }
  }
  let selected=selected.ok_or(Error::Invalid)?;
  self.editor.replace_line(&selected.bytes[..selected.len])?;
  self.history_cursor=None;
  self.history_draft.fill(0);
  self.history_draft_len=0;
  Ok(selected.len)
 }
}
#[cfg(test)]
mod r063_history_completion_tests{
 use super::*;
 fn filled()->InputLine{
  let mut input=InputLine::new();
  let mut result=[0u8;MAX_LINE];
  for cmd in [b"git status".as_slice(),b"cargo test",b"git diff"]{
   input.set_line(cmd).unwrap();
   input.feed(b'\n',&mut result).unwrap();
  }
  input
 }
 #[test]fn unique_history_completion_changes_editable_draft_not_command_execution(){
  let mut input=filled();
  input.set_line(b"cargo t").unwrap();
  assert_eq!(input.complete_from_history(),Ok(10));
  assert_eq!(input.bytes(),b"cargo test");
  assert_eq!(input.caret(),10);
  assert_eq!(input.history_count(),3);
  let mut out=[0u8;MAX_LINE];
  assert_eq!(input.feed(b'\n',&mut out),Ok(Event::Submitted(10)));
  assert_eq!(&out[..10],b"cargo test");
 }
 #[test]fn ambiguous_unknown_and_empty_prefix_preserve_unsent_draft(){
  let mut input=filled();
  input.set_line(b"git ").unwrap();
  assert_eq!(input.complete_from_history(),Err(Error::Invalid));
  assert_eq!(input.bytes(),b"git ");
  input.set_line(b"nosuch").unwrap();
  assert_eq!(input.complete_from_history(),Err(Error::Invalid));
  assert_eq!(input.bytes(),b"nosuch");
  input.set_line(b"").unwrap();
  assert_eq!(input.complete_from_history(),Err(Error::Invalid));
  assert_eq!(input.history_count(),3);
 }
 #[test]fn successful_recall_does_not_discard_protected_clipboard_or_history(){
  let mut input=filled();
  input.set_line(b"git status").unwrap();
  input.copy_range(0,3).unwrap();
  input.set_line(b"cargo t").unwrap();
  assert_eq!(input.complete_from_history(),Ok(10));
  assert_eq!(input.clipboard_bytes(),b"git");
  assert_eq!(input.history_previous(),Ok(8));
  assert_eq!(input.bytes(),b"git diff");
  assert_eq!(input.history_next(),Ok(10));
  assert_eq!(input.bytes(),b"cargo test");
 }
}


/// R064: expand a command draft to the LONGEST prefix shared by all
/// matching successfully submitted commands. Unlike unique completion,
/// this safely advances ambiguous drafts (for example, "git st" to
/// "git sta") but NEVER submits any command. No history, clipboard or
/// active command changes when there is no new shared character.
impl InputLine{
 pub fn complete_common_history_prefix(&mut self)->Result<usize,Error>{
  let prefix=self.editor.bytes();
  if prefix.is_empty()||prefix.iter().any(|&b|!(b' '..=b'~').contains(&b)){
   return Err(Error::Invalid);
  }
  let mut common=[0u8;MAX_LINE];
  let mut common_len=0usize;
  let mut found=false;
  for entry in self.history.iter().flatten(){
   if !entry.bytes[..entry.len].starts_with(prefix){continue;}
   if !found{
    common_len=entry.len;
    common[..common_len].copy_from_slice(&entry.bytes[..common_len]);
    found=true;
   }else{
    let mut n=0usize;
    while n<common_len&&n<entry.len&&common[n]==entry.bytes[n]{n+=1;}
    common_len=n;
   }
  }
  if !found||common_len<=prefix.len(){return Err(Error::Invalid);}
  self.editor.replace_line(&common[..common_len])?;
  self.history_cursor=None;
  self.history_draft.fill(0);
  self.history_draft_len=0;
  Ok(common_len)
 }
}
#[cfg(test)]
mod r064_shared_history_completion_tests{
 use super::*;
 fn with_history()->InputLine{
  let mut input=InputLine::new();let mut out=[0u8;MAX_LINE];
  for command in [b"git status".as_slice(),b"git stash",b"cargo test"]{
   input.set_line(command).unwrap();
   input.feed(b'\n',&mut out).unwrap();
  }
  input
 }
 #[test]fn ambiguous_git_commands_complete_only_the_shared_prefix(){
  let mut input=with_history();
  input.set_line(b"git st").unwrap();
  assert_eq!(input.complete_common_history_prefix(),Ok(7));
  assert_eq!(input.bytes(),b"git sta");
  assert_eq!(input.caret(),7);
  assert_eq!(input.history_count(),3);
  assert_eq!(input.complete_common_history_prefix(),Err(Error::Invalid));
  assert_eq!(input.bytes(),b"git sta");
 }
 #[test]fn unique_match_extends_draft_without_implicit_enter(){
  let mut input=with_history();
  input.set_line(b"cargo t").unwrap();
  assert_eq!(input.complete_common_history_prefix(),Ok(10));
  assert_eq!(input.bytes(),b"cargo test");
  assert_eq!(input.history_count(),3);
  let mut out=[0u8;MAX_LINE];
  assert_eq!(input.feed(b'\n',&mut out),Ok(Event::Submitted(10)));
  assert_eq!(&out[..10],b"cargo test");
 }
 #[test]fn unknown_exact_and_empty_prefixes_preserve_clipboard_and_draft(){
  let mut input=with_history();
  input.set_line(b"git status").unwrap();
  input.copy_range(0,3).unwrap();
  assert_eq!(input.complete_common_history_prefix(),Err(Error::Invalid));
  input.set_line(b"unknown").unwrap();
  assert_eq!(input.complete_common_history_prefix(),Err(Error::Invalid));
  assert_eq!(input.bytes(),b"unknown");
  input.set_line(b"").unwrap();
  assert_eq!(input.complete_common_history_prefix(),Err(Error::Invalid));
  assert_eq!(input.clipboard_bytes(),b"git");
  assert_eq!(input.history_count(),3);
 }
}


/// R065: privacy-aware removal of exact successful commands from a
/// process-local fixed history ring. Unknown/invalid commands are a no-op.
/// While browsing, restore the unsent draft BEFORE compacting history so
/// the user never loses their work or executes a recalled command.
/// The caller must bind this InputLine to the authenticated process.
impl InputLine{
 pub fn forget_history_command(&mut self,command:&[u8])->Result<usize,Error>{
  if command.is_empty()||command.len()>MAX_LINE||
     command.iter().any(|&b|!(b' '..=b'~').contains(&b)){
   return Err(Error::Invalid);
  }
  let removed=self.history.iter().flatten().filter(|entry|
   &entry.bytes[..entry.len]==command).count();
  if removed==0{return Err(Error::Invalid);}
  // replace_line cannot fail: this draft was previously captured from
  // the same fixed-capacity valid LineEditor.
  if self.history_cursor.is_some(){
   self.editor.replace_line(&self.history_draft[..self.history_draft_len])?;
  }
  let mut retained=[None;HISTORY_LIMIT];
  let mut size=0usize;
  for entry in self.history.iter().flatten(){
   if &entry.bytes[..entry.len]!=command{
    retained[size]=Some(*entry);size+=1;
   }
  }
  self.history=retained;
  self.history_len=size;
  self.history_cursor=None;
  self.history_draft.fill(0);
  self.history_draft_len=0;
  Ok(removed)
 }
}
#[cfg(test)]
mod r065_history_redaction_tests{
 use super::*;
 fn filled()->InputLine{
  let mut input=InputLine::new();let mut out=[0u8;MAX_LINE];
  for command in [b"git status".as_slice(),b"cargo test",b"git diff"]{
   input.set_line(command).unwrap();
   input.feed(b'\n',&mut out).unwrap();
  }
  input
 }
 #[test]fn removing_middle_command_preserves_newest_oldest_and_draft(){
  let mut input=filled();
  input.set_line(b"draft").unwrap();
  assert_eq!(input.forget_history_command(b"cargo test"),Ok(1));
  assert_eq!(input.history_count(),2);
  assert_eq!(input.bytes(),b"draft");
  assert_eq!(input.history_previous(),Ok(8));
  assert_eq!(input.bytes(),b"git diff");
  assert_eq!(input.history_previous(),Ok(10));
  assert_eq!(input.bytes(),b"git status");
  assert_eq!(input.history_next(),Ok(8));
  assert_eq!(input.history_next(),Ok(5));
  assert_eq!(input.bytes(),b"draft");
 }
 #[test]fn redacting_active_history_entry_restores_pending_unsent_text(){
  let mut input=filled();
  input.set_line(b"unsent work").unwrap();
  assert_eq!(input.history_previous(),Ok(8));
  assert_eq!(input.bytes(),b"git diff");
  assert_eq!(input.forget_history_command(b"git diff"),Ok(1));
  assert_eq!(input.bytes(),b"unsent work");
  assert_eq!(input.history_count(),2);
  assert_eq!(input.history_next(),Err(Error::Invalid));
  assert_eq!(input.history_previous(),Ok(10));
  assert_eq!(input.bytes(),b"cargo test");
 }
 #[test]fn exact_matches_only_and_invalid_requests_do_not_modify_clipboard(){
  let mut input=filled();
  input.set_line(b"git status").unwrap();
  input.copy_range(0,3).unwrap();
  assert_eq!(input.forget_history_command(b"git"),Err(Error::Invalid));
  assert_eq!(input.forget_history_command(b""),Err(Error::Invalid));
  assert_eq!(input.forget_history_command(b"bad\ncommand"),Err(Error::Invalid));
  assert_eq!(input.history_count(),3);
  assert_eq!(input.clipboard_bytes(),b"git");
  assert_eq!(input.forget_history_command(b"git status"),Ok(1));
  assert_eq!(input.history_count(),2);
  assert_eq!(input.bytes(),b"git status");
  input.forget();
  assert_eq!(input.history_count(),0);
  assert_eq!(input.clipboard_bytes(),b"");
 }
}


/// R066: insert a bounded fragment of a previously *successfully submitted*
/// command at the CURRENT caret of a private unsent Terminal draft. This is
/// a deliberate editing operation, not submission or clipboard access.
/// Capacity, history index, fragment bounds and printable input are
/// checked before mutating the live line; errors leave browsing state alone.
impl InputLine{
 pub fn paste_history_fragment(&mut self,index:usize,
    from:usize,to:usize)->Result<usize,Error>{
  let entry=self.history.get(index).and_then(|value|*value)
    .ok_or(Error::Invalid)?;
  if index>=self.history_len||from>=to||to>entry.len{
   return Err(Error::Invalid);
  }
  let bytes=&entry.bytes[from..to];
  if bytes.iter().any(|&b|!(b' '..=b'~').contains(&b)){
   return Err(Error::Invalid);
  }
  self.editor.paste(bytes)?;
  self.history_cursor=None;
  self.history_draft.fill(0);
  self.history_draft_len=0;
  Ok(bytes.len())
 }
}
#[cfg(test)]
mod r066_history_fragment_tests{
 use super::*;
 fn ready()->InputLine{
  let mut input=InputLine::new();
  let mut out=[0u8;MAX_LINE];
  for cmd in [b"git status".as_slice(),b"cargo test",b"git diff"]{
   input.set_line(cmd).unwrap();
   input.feed(b'\n',&mut out).unwrap();
  }
  input
 }
 #[test]fn inserts_a_real_command_fragment_at_caret_without_executing(){
  let mut input=ready();
  input.set_line(b"run ").unwrap();
  assert_eq!(input.paste_history_fragment(1,6,10),Ok(4));
  assert_eq!(input.bytes(),b"run test");
  assert_eq!(input.history_count(),3);
  assert_eq!(input.paste_history_fragment(0,0,3),Ok(3));
  assert_eq!(input.bytes(),b"run testgit");
  let mut out=[0u8;MAX_LINE];
  assert_eq!(input.feed(b'\n',&mut out),Ok(Event::Submitted(11)));
  assert_eq!(&out[..11],b"run testgit");
 }
 #[test]fn full_line_and_invalid_range_do_not_mutate_history_or_clipboard(){
  let mut input=ready();
  input.set_line(b"git status").unwrap();
  input.copy_range(0,3).unwrap();
  input.set_line(&[b'x';MAX_LINE]).unwrap();
  assert_eq!(input.paste_history_fragment(1,0,5),Err(Error::Full));
  assert_eq!(input.bytes(),&[b'x';MAX_LINE]);
  for (index,from,to) in [(3,0,1),(0,1,1),(1,8,20),(1,10,11)]{
   assert_eq!(input.paste_history_fragment(index,from,to),
     Err(Error::Invalid));
  }
  assert_eq!(input.history_count(),3);
  assert_eq!(input.clipboard_bytes(),b"git");
 }
 #[test]fn insertion_after_recall_is_still_an_unsent_explicit_edit(){
  let mut input=ready();
  input.set_line(b"draft").unwrap();
  assert_eq!(input.history_previous(),Ok(8));
  assert_eq!(input.bytes(),b"git diff");
  assert_eq!(input.paste_history_fragment(0,0,3),Ok(3));
  assert_eq!(input.bytes(),b"git diffgit");
  assert_eq!(input.history_next(),Err(Error::Invalid));
  assert_eq!(input.history_count(),3);
 }
}


/// R067: history-based inline ghost text. Suggest the remainder of the
/// most recent successful command matching the unsent draft. Nothing is
/// inserted, submitted or moved; UI can explicitly accept the suggestion.
impl InputLine{
 pub fn suggest_history_suffix(&self,out:&mut[u8])->Result<usize,Error>{
  let prefix=self.editor.bytes();
  if prefix.is_empty(){return Err(Error::Invalid);}
  let candidate=self.history[..self.history_len].iter().rev().flatten()
   .find(|entry|entry.len>prefix.len()&&
     entry.bytes[..entry.len].starts_with(prefix))
   .ok_or(Error::Invalid)?;
  let suggestion=&candidate.bytes[prefix.len()..candidate.len];
  if suggestion.len()>out.len(){return Err(Error::TooSmall);}
  out[..suggestion.len()].copy_from_slice(suggestion);
  Ok(suggestion.len())
 }
 /// Only a separate explicit caller action accepts ghost text; this never
 /// submits a command or alters the process-private clipboard/history.
 pub fn accept_history_suffix(&mut self)->Result<usize,Error>{
  let mut suffix=[0u8;MAX_LINE];
  let n=self.suggest_history_suffix(&mut suffix)?;
  let next=self.length().checked_add(n).ok_or(Error::Full)?;
  if next>MAX_LINE{return Err(Error::Full);}
  let mut staged=[0u8;MAX_LINE];
  staged[..self.length()].copy_from_slice(self.bytes());
  staged[self.length()..next].copy_from_slice(&suffix[..n]);
  self.editor.replace_line(&staged[..next])?;
  self.history_cursor=None;
  self.history_draft.fill(0);self.history_draft_len=0;
  Ok(next)
 }
}
#[cfg(test)]
mod r067_history_suggestion_tests{
 use super::*;
 fn sample()->InputLine{
  let mut input=InputLine::new();let mut out=[0u8;MAX_LINE];
  for cmd in [b"git status".as_slice(),b"cargo test",b"git diff"]{
   input.set_line(cmd).unwrap();input.feed(b'\n',&mut out).unwrap();
  }
  input
 }
 #[test]fn suggestion_only_reads_newest_matching_history_and_accepts_explicitly(){
  let mut input=sample();
  input.set_line(b"git ").unwrap();
  let mut out=[0u8;MAX_LINE];
  assert_eq!(input.suggest_history_suffix(&mut out),Ok(4));
  assert_eq!(&out[..4],b"diff");
  assert_eq!(input.bytes(),b"git ");
  assert_eq!(input.accept_history_suffix(),Ok(8));
  assert_eq!(input.bytes(),b"git diff");
  assert_eq!(input.history_count(),3);
 }
 #[test]fn short_buffer_and_unknown_prefix_preserve_caller_output_and_draft(){
  let mut input=sample();input.set_line(b"cargo ").unwrap();
  let mut short=[0xa5u8;3];
  assert_eq!(input.suggest_history_suffix(&mut short),Err(Error::TooSmall));
  assert_eq!(short,[0xa5;3]);
  input.set_line(b"unknown").unwrap();
  assert_eq!(input.suggest_history_suffix(&mut short),Err(Error::Invalid));
  assert_eq!(input.bytes(),b"unknown");
 }
 #[test]fn empty_and_fully_matched_history_do_not_modify_input(){
  let mut input=sample();input.set_line(b"").unwrap();
  assert_eq!(input.accept_history_suffix(),Err(Error::Invalid));
  input.set_line(b"cargo test").unwrap();
  assert_eq!(input.accept_history_suffix(),Err(Error::Invalid));
  assert_eq!(input.bytes(),b"cargo test");
 }
}


/// R068: delete the next shell word from the live keyboard caret without
/// executing a command or changing the private clipboard/history.
impl LineEditor{
 pub fn erase_next_word(&mut self)->Event{
  let start=self.caret;
  while self.caret<self.len&&self.line[self.caret]==b' '{self.caret+=1;}
  while self.caret<self.len&&self.line[self.caret]!=b' '{self.caret+=1;}
  let end=self.caret;
  if start==end{return Event::Empty;}
  self.line.copy_within(end..self.len,start);
  self.len-=end-start;
  self.line[self.len..].fill(0);
  self.caret=start;
  Event::Erased
 }
}
impl InputLine{
 pub fn erase_next_word(&mut self)->Event{
  let outcome=self.editor.erase_next_word();
  if outcome==Event::Erased{
   self.history_cursor=None;
   self.history_draft.fill(0);self.history_draft_len=0;
  }
  outcome
 }
}
#[cfg(test)]
mod r068_forward_word_edit_tests{
 use super::*;
 #[test]fn delete_suffix_of_word_at_live_caret_preserves_past_characters(){
  let mut input=InputLine::new();
  input.set_line(b"git status --short").unwrap();
  for _ in 0..4{input.action(EditKey::Left).unwrap();}
  assert_eq!(input.caret(),14);
  assert_eq!(input.erase_next_word(),Event::Erased);
  assert_eq!(input.bytes(),b"git status --s");
  assert_eq!(input.caret(),14);
 }
 #[test]fn delete_word_and_whitespace_preserving_other_arguments(){
  let mut input=InputLine::new();
  input.set_line(b"git  status --short").unwrap();
  input.action(EditKey::Home).unwrap();
  assert_eq!(input.erase_next_word(),Event::Erased);
  assert_eq!(input.bytes(),b"  status --short");
  assert_eq!(input.erase_next_word(),Event::Erased);
  assert_eq!(input.bytes(),b" --short");
  input.action(EditKey::End).unwrap();
  assert_eq!(input.erase_next_word(),Event::Empty);
 }
 #[test]fn empty_and_final_caret_are_noops_and_never_submit(){
  let mut input=InputLine::new();
  assert_eq!(input.erase_next_word(),Event::Empty);
  input.set_line(b"  ").unwrap();
  input.action(EditKey::End).unwrap();
  assert_eq!(input.erase_next_word(),Event::Empty);
  assert_eq!(input.bytes(),b"  ");
  input.action(EditKey::Home).unwrap();
  assert_eq!(input.erase_next_word(),Event::Erased);
  assert!(input.bytes().is_empty());
  assert_eq!(input.history_count(),0);
 }
}


/// R069: Ctrl-U-style removal from the beginning of the draft through the
/// live caret. It never submits the command, mutates history or clipboard,
/// or leaves the erased bytes in the unused backing buffer.
impl LineEditor {
    pub fn erase_to_home(&mut self) -> Event {
        if self.caret == 0 { return Event::Empty; }
        let count = self.caret;
        self.line.copy_within(count..self.len, 0);
        self.len -= count;
        self.caret = 0;
        self.line[self.len..].fill(0);
        Event::Erased
    }
}
impl InputLine {
    pub fn erase_to_home(&mut self) -> Event {
        let result = self.editor.erase_to_home();
        if result == Event::Erased {
            self.history_cursor = None;
            self.history_draft.fill(0);
            self.history_draft_len = 0;
        }
        result
    }
}
#[cfg(test)]
mod r069_erase_to_home_tests {
    use super::*;
    #[test]
    fn deletes_only_the_prefix_and_preserves_suffix_and_cursor() {
        let mut input = InputLine::new();
        input.set_line(b"cargo build").unwrap();
        input.action(EditKey::Home).unwrap();
        for _ in 0..6 { input.action(EditKey::Right).unwrap(); }
        assert_eq!(input.erase_to_home(), Event::Erased);
        assert_eq!(input.bytes(), b"build");
        assert_eq!(input.caret(), 0);
        assert_eq!(input.history_count(), 0);
    }
    #[test]
    fn home_is_noop_and_full_draft_is_scrubbed() {
        let mut input = InputLine::new();
        input.set_line(b"git status").unwrap();
        input.action(EditKey::Home).unwrap();
        assert_eq!(input.erase_to_home(), Event::Empty);
        assert_eq!(input.bytes(), b"git status");
        input.action(EditKey::End).unwrap();
        assert_eq!(input.erase_to_home(), Event::Erased);
        assert!(input.bytes().is_empty());
        assert_eq!(input.caret(), 0);
        assert_eq!(input.erase_to_home(), Event::Empty);
    }
    #[test]
    fn clipboard_and_successful_history_survive_unsent_erasure() {
        let mut input = InputLine::new();
        let mut out = [0u8; MAX_LINE];
        input.set_line(b"echo ok").unwrap();
        assert_eq!(input.feed(b'\n', &mut out), Ok(Event::Submitted(7)));
        input.set_line(b"paste this").unwrap();
        input.copy_range(0, 5).unwrap();
        input.action(EditKey::End).unwrap();
        assert_eq!(input.erase_to_home(), Event::Erased);
        assert_eq!(input.history_count(), 1);
        assert_eq!(input.clipboard_bytes(), b"paste");
        assert!(input.bytes().is_empty());
    }
}


/// R070: repair the shell command verb without changing the case of path
/// arguments or a literal payload. No command is submitted by this action.
impl LineEditor {
    pub fn lowercase_command_verb(&mut self) -> Event {
        let start = match self.line[..self.len].iter().position(|b| *b != b' ') {
            Some(n) => n,
            None => return Event::Empty,
        };
        let end = start + self.line[start..self.len].iter()
            .position(|b| *b == b' ').unwrap_or(self.len - start);
        let mut changed = false;
        for byte in &mut self.line[start..end] {
            if byte.is_ascii_uppercase() {
                *byte = byte.to_ascii_lowercase();
                changed = true;
            }
        }
        if changed { Event::Typed } else { Event::Empty }
    }
}
impl InputLine {
    pub fn lowercase_command_verb(&mut self) -> Event {
        let result = self.editor.lowercase_command_verb();
        if result == Event::Typed {
            self.history_cursor = None;
            self.history_draft.fill(0);
            self.history_draft_len = 0;
        }
        result
    }
}

#[cfg(test)]
mod r070_shell_verb_edit_tests {
    use super::*;
    #[test]
    fn lowercases_only_verb_without_submitting_or_changing_caret() {
        let mut line = InputLine::new();
        line.set_line(b"  HEADLINES /MixedCase N").unwrap();
        line.action(EditKey::Home).unwrap();
        assert_eq!(line.lowercase_command_verb(), Event::Typed);
        assert_eq!(line.bytes(), b"  headlines /MixedCase N");
        assert_eq!(line.caret(), 0);
        assert_eq!(line.lowercase_command_verb(), Event::Empty);
        assert_eq!(line.history_count(), 0);
    }
    #[test]
    fn command_history_and_clipboard_stay_intact() {
        let mut line = InputLine::new();
        let mut submission = [0u8; MAX_LINE];
        line.set_line(b"pwd").unwrap();
        line.feed(b'\n', &mut submission).unwrap();
        line.set_line(b"CAT /Secret").unwrap();
        line.copy_range(0, 3).unwrap();
        assert_eq!(line.lowercase_command_verb(), Event::Typed);
        assert_eq!(line.bytes(), b"cat /Secret");
        assert_eq!(line.clipboard_bytes(), b"CAT");
        assert_eq!(line.history_count(), 1);
        line.set_line(b"   ").unwrap();
        assert_eq!(line.lowercase_command_verb(), Event::Empty);
    }
}


/// R071: move an interactive shell caret directly to indexed whitespace-
/// delimited argument. Index 0 denotes the command verb; navigation
/// preserves the editable line, clipboard and submitted command history.
impl LineEditor {
    pub fn jump_to_argument(&mut self, index: usize) -> Result<usize, Error> {
        let mut at = 0usize;
        let mut token = 0usize;
        while at < self.len {
            while at < self.len && self.line[at] == b' ' { at += 1; }
            if at == self.len { break; }
            if token == index {
                self.caret = at;
                return Ok(at);
            }
            token = token.checked_add(1).ok_or(Error::Invalid)?;
            while at < self.len && self.line[at] != b' ' { at += 1; }
        }
        Err(Error::Invalid)
    }
}
impl InputLine {
    pub fn jump_to_argument(&mut self, index: usize) -> Result<usize, Error> {
        self.editor.jump_to_argument(index)
    }
}

#[cfg(test)]
mod r071_terminal_argument_navigation_tests {
    use super::*;
    #[test]
    fn navigates_real_draft_without_mutating_arguments() {
        let mut input = InputLine::new();
        input.set_line(b"  linespan  /Mixed  2   4 ").unwrap();
        assert_eq!(input.jump_to_argument(0), Ok(2));
        assert_eq!(input.jump_to_argument(1), Ok(12));
        assert_eq!(input.jump_to_argument(2), Ok(20));
        assert_eq!(input.jump_to_argument(3), Ok(24));
        assert_eq!(input.caret(), 24);
        assert_eq!(input.bytes(), b"  linespan  /Mixed  2   4 ");
        assert_eq!(input.history_count(), 0);
    }
    #[test]
    fn invalid_token_keeps_caret_clipboard_and_history() {
        let mut input = InputLine::new();
        let mut out = [0u8; MAX_LINE];
        input.set_line(b"pwd").unwrap();
        input.feed(b'\n', &mut out).unwrap();
        input.set_line(b"cat /src").unwrap();
        input.copy_range(0, 3).unwrap();
        input.jump_to_argument(1).unwrap();
        let previous = input.caret();
        assert_eq!(input.jump_to_argument(99), Err(Error::Invalid));
        assert_eq!(input.caret(), previous);
        assert_eq!(input.clipboard_bytes(), b"cat");
        assert_eq!(input.history_count(), 1);
        input.set_line(b"  ").unwrap();
        assert_eq!(input.jump_to_argument(0), Err(Error::Invalid));
    }
}


/// R072: reorder two adjacent whitespace-delimited command tokens in the
/// live unsent draft. The exact intervening whitespace and surrounding
/// tokens survive, with a single bounded, non-submitting mutation.
impl LineEditor {
    pub fn swap_adjacent_arguments(&mut self, first: usize) -> Result<Event, Error> {
        let second = first.checked_add(1).ok_or(Error::Invalid)?;
        let mut cursor = 0usize;
        let mut token = 0usize;
        let mut a = None;
        let mut b = None;
        while cursor < self.len {
            while cursor < self.len && self.line[cursor] == b' ' { cursor += 1; }
            if cursor == self.len { break; }
            let begin = cursor;
            while cursor < self.len && self.line[cursor] != b' ' { cursor += 1; }
            if token == first { a = Some((begin, cursor)); }
            if token == second { b = Some((begin, cursor)); break; }
            token += 1;
        }
        let (a_start, a_end) = a.ok_or(Error::Invalid)?;
        let (b_start, b_end) = b.ok_or(Error::Invalid)?;
        let left = &self.line[a_start..a_end];
        let right = &self.line[b_start..b_end];
        if left == right { return Ok(Event::Empty); }
        let mut staged = [0u8; MAX_LINE];
        staged[..a_start].copy_from_slice(&self.line[..a_start]);
        let mut at = a_start;
        staged[at..at+right.len()].copy_from_slice(right);
        at += right.len();
        staged[at..at+b_start-a_end].copy_from_slice(&self.line[a_end..b_start]);
        at += b_start-a_end;
        staged[at..at+left.len()].copy_from_slice(left);
        at += left.len();
        staged[at..self.len].copy_from_slice(&self.line[b_end..self.len]);
        let moved_len = right.len();
        self.line = staged;
        self.caret = a_start + moved_len;
        Ok(Event::Typed)
    }
}
impl InputLine {
    pub fn swap_adjacent_arguments(&mut self, first: usize) -> Result<Event, Error> {
        let result = self.editor.swap_adjacent_arguments(first)?;
        if result == Event::Typed {
            self.history_cursor = None;
            self.history_draft.fill(0);
            self.history_draft_len = 0;
        }
        Ok(result)
    }
}

#[cfg(test)]
mod r072_shell_argument_reorder_tests {
    use super::*;
    #[test]
    fn swaps_arguments_and_preserves_exact_delimiters_and_history() {
        let mut line = InputLine::new();
        line.set_line(b"git  checkout   main  ").unwrap();
        assert_eq!(line.swap_adjacent_arguments(1), Ok(Event::Typed));
        assert_eq!(line.bytes(), b"git  main   checkout  ");
        assert_eq!(line.caret(), 9);
        assert_eq!(line.history_count(), 0);
    }
    #[test]
    fn missing_token_equal_token_and_clipboard_are_fail_closed() {
        let mut line = InputLine::new();
        let mut out = [0u8; MAX_LINE];
        line.set_line(b"pwd").unwrap();
        line.feed(b'\n', &mut out).unwrap();
        line.set_line(b"run one one").unwrap();
        line.copy_range(4, 7).unwrap();
        assert_eq!(line.swap_adjacent_arguments(1), Ok(Event::Empty));
        assert_eq!(line.bytes(), b"run one one");
        assert_eq!(line.swap_adjacent_arguments(2), Err(Error::Invalid));
        assert_eq!(line.swap_adjacent_arguments(usize::MAX), Err(Error::Invalid));
        assert_eq!(line.clipboard_bytes(), b"one");
        assert_eq!(line.history_count(), 1);
    }
}


/// R073: replace exactly one whitespace-separated command argument while
/// preserving the verb, other arguments, spacing and private clipboard.
/// No shell execution occurs, and preflight protects the current draft.
impl LineEditor {
    pub fn replace_argument(&mut self, index: usize, replacement: &[u8])
        -> Result<Event, Error> {
        if replacement.is_empty() || replacement.iter().any(|b| !b.is_ascii_graphic()) {
            return Err(Error::Invalid);
        }
        let mut cursor = 0usize;
        let mut token = 0usize;
        while cursor < self.len {
            while cursor < self.len && self.line[cursor] == b' ' { cursor += 1; }
            if cursor == self.len { break; }
            let from = cursor;
            while cursor < self.len && self.line[cursor] != b' ' { cursor += 1; }
            if token == index {
                if self.line[from..cursor] == *replacement { return Ok(Event::Empty); }
                return self.replace_range(from, cursor, replacement);
            }
            token = token.checked_add(1).ok_or(Error::Invalid)?;
        }
        Err(Error::Invalid)
    }
}
impl InputLine {
    pub fn replace_argument(&mut self, index: usize, replacement: &[u8])
        -> Result<Event, Error> {
        let changed = self.editor.replace_argument(index, replacement)?;
        if changed != Event::Empty {
            self.history_cursor = None;
            self.history_draft.fill(0);
            self.history_draft_len = 0;
        }
        Ok(changed)
    }
}

#[cfg(test)]
mod r073_argument_replacement_tests {
    use super::*;
    #[test]
    fn replaces_only_selected_token_and_keeps_history_clipboard() {
        let mut line = InputLine::new();
        let mut submitted = [0u8; MAX_LINE];
        line.set_line(b"pwd").unwrap();
        line.feed(b'\n', &mut submitted).unwrap();
        line.set_line(b"git  checkout  OldPath  ").unwrap();
        line.copy_range(4, 13).unwrap();
        assert_eq!(line.replace_argument(2, b"new/Path"), Ok(Event::Typed));
        assert_eq!(line.bytes(), b"git  checkout  new/Path  ");
        assert_eq!(line.caret(), "git  checkout  new/Path".len());
        assert_eq!(line.clipboard_bytes(), b" checkout");
        assert_eq!(line.history_count(), 1);
        assert_eq!(line.replace_argument(2, b"new/Path"), Ok(Event::Empty));
    }
    #[test]
    fn bad_argument_invalid_bytes_and_capacity_are_atomic() {
        let mut line = InputLine::new();
        line.set_line(b"write /file old").unwrap();
        let caret = line.caret();
        assert_eq!(line.replace_argument(100, b"x"), Err(Error::Invalid));
        assert_eq!(line.replace_argument(2, b"two words"), Err(Error::Invalid));
        assert_eq!(line.replace_argument(2, b"\n"), Err(Error::Invalid));
        assert_eq!(line.replace_argument(2, &[b'x'; MAX_LINE]), Err(Error::Full));
        assert_eq!(line.bytes(), b"write /file old");
        assert_eq!(line.caret(), caret);
    }
}


/// R074: delete one indexed whitespace-delimited argument from the unsent
/// terminal draft, removing the appropriate adjacent separator without
/// destroying unrelated tokens, history or the private clipboard.
impl LineEditor {
    pub fn delete_argument(&mut self, index: usize) -> Result<Event, Error> {
        let mut at = 0usize;
        let mut ordinal = 0usize;
        let mut match_range = None;
        while at < self.len {
            while at < self.len && self.line[at] == b' ' { at += 1; }
            if at == self.len { break; }
            let start = at;
            while at < self.len && self.line[at] != b' ' { at += 1; }
            let end = at;
            if ordinal == index { match_range = Some((start, end)); break; }
            ordinal = ordinal.checked_add(1).ok_or(Error::Invalid)?;
        }
        let (start, end) = match_range.ok_or(Error::Invalid)?;
        // If another token follows, consume our following spaces; otherwise
        // remove our preceding spaces and any trailing blanks as well.
        let mut after = end;
        while after < self.len && self.line[after] == b' ' { after += 1; }
        if after < self.len {
            self.replace_range(start, after, b"")
        } else {
            let mut before = start;
            while before > 0 && self.line[before-1] == b' ' { before -= 1; }
            self.replace_range(before, self.len, b"")
        }
    }
}
impl InputLine {
    pub fn delete_argument(&mut self, index: usize) -> Result<Event, Error> {
        let action = self.editor.delete_argument(index)?;
        if action != Event::Empty {
            self.history_cursor = None;
            self.history_draft.fill(0);
            self.history_draft_len = 0;
        }
        Ok(action)
    }
}

#[cfg(test)]
mod r074_terminal_token_delete_tests {
    use super::*;
    #[test]
    fn deletes_middle_final_and_command_tokens_preserving_neighbors() {
        let mut input = InputLine::new();
        input.set_line(b"git  checkout   MyFile  now").unwrap();
        assert_eq!(input.delete_argument(1), Ok(Event::Erased));
        assert_eq!(input.bytes(), b"git  MyFile  now");
        assert_eq!(input.delete_argument(2), Ok(Event::Erased));
        assert_eq!(input.bytes(), b"git  MyFile");
        assert_eq!(input.delete_argument(0), Ok(Event::Erased));
        assert_eq!(input.bytes(), b"MyFile");
        assert_eq!(input.delete_argument(0), Ok(Event::Erased));
        assert_eq!(input.bytes(), b"");
    }
    #[test]
    fn invalid_index_preserves_input_cursor_history_and_clipboard() {
        let mut input = InputLine::new();
        let mut submitted = [0u8; MAX_LINE];
        input.set_line(b"pwd").unwrap();
        input.feed(b'\n', &mut submitted).unwrap();
        input.set_line(b"cat /source").unwrap();
        input.copy_range(4, 11).unwrap();
        let caret = input.caret();
        assert_eq!(input.delete_argument(3), Err(Error::Invalid));
        assert_eq!(input.delete_argument(usize::MAX), Err(Error::Invalid));
        assert_eq!(input.bytes(), b"cat /source");
        assert_eq!(input.caret(), caret);
        assert_eq!(input.history_count(), 1);
        assert_eq!(input.clipboard_bytes(), b"/source");
        input.set_line(b"    ").unwrap();
        assert_eq!(input.delete_argument(0), Err(Error::Invalid));
    }
}


/// R075: duplicate a whitespace-delimited command argument in the live,
/// unsent terminal draft. This never issues a command or copies its value
/// into the clipboard/history. All insertion capacity is checked first.
impl LineEditor {
    pub fn duplicate_argument(&mut self, index: usize) -> Result<Event, Error> {
        let mut at = 0usize;
        let mut token = 0usize;
        while at < self.len {
            while at < self.len && self.line[at] == b' ' { at += 1; }
            if at == self.len { break; }
            let start = at;
            while at < self.len && self.line[at] != b' ' { at += 1; }
            if token == index {
                let width = at - start;
                let new_size = self.len.checked_add(width+1).ok_or(Error::Full)?;
                if new_size > MAX_LINE { return Err(Error::Full); }
                let mut payload = [0u8; MAX_LINE];
                payload[0] = b' ';
                payload[1..width+1].copy_from_slice(&self.line[start..at]);
                return self.replace_range(at, at, &payload[..width+1]);
            }
            token = token.checked_add(1).ok_or(Error::Invalid)?;
        }
        Err(Error::Invalid)
    }
}
impl InputLine {
    pub fn duplicate_argument(&mut self, index: usize) -> Result<Event, Error> {
        let result = self.editor.duplicate_argument(index)?;
        if result != Event::Empty {
            self.history_cursor = None;
            self.history_draft.fill(0);
            self.history_draft_len = 0;
        }
        Ok(result)
    }
}

#[cfg(test)]
mod r075_duplicate_shell_argument_tests {
    use super::*;
    #[test]
    fn duplicates_selected_token_without_changing_case_clipboard_or_history() {
        let mut line = InputLine::new();
        let mut submitted = [0u8; MAX_LINE];
        line.set_line(b"pwd").unwrap();
        line.feed(b'\n', &mut submitted).unwrap();
        line.set_line(b"git status  /MixedCase").unwrap();
        line.copy_range(4, 10).unwrap();
        assert_eq!(line.duplicate_argument(1), Ok(Event::Typed));
        assert_eq!(line.bytes(), b"git status status  /MixedCase");
        assert_eq!(line.caret(), b"git status status".len());
        assert_eq!(line.clipboard_bytes(), b"status");
        assert_eq!(line.history_count(), 1);
    }
    #[test]
    fn missing_token_and_insufficient_capacity_preserve_draft() {
        let mut line = InputLine::new();
        line.set_line(b"run a").unwrap();
        let before = line.caret();
        assert_eq!(line.duplicate_argument(3), Err(Error::Invalid));
        assert_eq!(line.bytes(), b"run a");
        assert_eq!(line.caret(), before);
        let full = [b'x'; MAX_LINE];
        line.set_line(&full).unwrap();
        assert_eq!(line.duplicate_argument(0), Err(Error::Full));
        assert_eq!(line.bytes(), &full);
    }
}


// R075 expanded workstream D: bounded transformations on unsent shell input.
impl LineEditor {
    /// Canonicalize only the space-separated argument separators. Paths and
    /// other case-sensitive argument bytes are never lowercased.
    pub fn compact_argument_spacing(&mut self) -> Result<Event, Error> {
        let mut staged = [0u8; MAX_LINE];
        let mut written = 0usize;
        let mut gap = false;
        for &byte in &self.line[..self.len] {
            if byte == b' ' {
                if written > 0 { gap = true; }
            } else {
                if gap { staged[written] = b' '; written += 1; gap = false; }
                staged[written] = byte;
                written += 1;
            }
        }
        if written == self.len && staged[..written] == self.line[..self.len] {
            return Ok(Event::Empty);
        }
        self.replace_line(&staged[..written])?;
        Ok(Event::Typed)
    }

    /// Insert an argument before token N, or at N==token count to append.
    /// No quoting/escaping is assumed: the terminal grammar uses ASCII spaces.
    pub fn insert_argument_before(&mut self, index: usize,
        argument: &[u8]) -> Result<Event, Error> {
        if argument.is_empty() || argument.iter().any(|b| !b.is_ascii_graphic()) {
            return Err(Error::Invalid);
        }
        let mut at = 0usize;
        let mut ordinal = 0usize;
        let mut insert = None;
        while at < self.len {
            while at < self.len && self.line[at] == b' ' { at += 1; }
            if at == self.len { break; }
            if ordinal == index { insert = Some(at); break; }
            while at < self.len && self.line[at] != b' ' { at += 1; }
            ordinal += 1;
        }
        if let Some(where_at) = insert {
            let mut data = [0u8; MAX_LINE];
            let required = argument.len()+1;
            if required > MAX_LINE { return Err(Error::Full); }
            data[..argument.len()].copy_from_slice(argument);
            data[argument.len()] = b' ';
            return self.replace_range(where_at, where_at, &data[..required]);
        }
        if index != ordinal { return Err(Error::Invalid); }
        let end = self.line[..self.len].iter().rposition(|b| *b != b' ')
            .map(|pos| pos+1).unwrap_or(0);
        let mut data = [0u8; MAX_LINE];
        let prefix = usize::from(end > 0);
        if argument.len()+prefix > MAX_LINE { return Err(Error::Full); }
        if prefix != 0 { data[0] = b' '; }
        data[prefix..prefix+argument.len()].copy_from_slice(argument);
        self.replace_range(end, end, &data[..prefix+argument.len()])
    }

    /// Move one argument to the final position, rebuilding the whitespace-
    /// delimited draft in a single bounded operation without executing it.
    pub fn move_argument_to_end(&mut self, index: usize) -> Result<Event, Error> {
        let mut spans = [(0usize, 0usize); 32];
        let mut count = 0usize;
        let mut at = 0usize;
        while at < self.len {
            while at < self.len && self.line[at] == b' ' { at += 1; }
            if at == self.len { break; }
            if count == spans.len() { return Err(Error::Full); }
            let start = at;
            while at < self.len && self.line[at] != b' ' { at += 1; }
            spans[count] = (start, at);
            count += 1;
        }
        if index >= count { return Err(Error::Invalid); }
        if index + 1 == count { return Ok(Event::Empty); }
        let mut staged = [0u8; MAX_LINE];
        let mut written = 0usize;
        for token in (0..count).filter(|n| *n != index).chain(core::iter::once(index)) {
            if written > 0 { staged[written] = b' '; written += 1; }
            let (start, end) = spans[token];
            staged[written..written+end-start].copy_from_slice(&self.line[start..end]);
            written += end-start;
        }
        self.replace_line(&staged[..written])?;
        Ok(Event::Typed)
    }
}
impl InputLine {
    pub fn compact_argument_spacing(&mut self) -> Result<Event, Error> {
        let result = self.editor.compact_argument_spacing()?;
        if result != Event::Empty {
            self.history_cursor = None;
            self.history_draft.fill(0);
            self.history_draft_len = 0;
        }
        Ok(result)
    }
    pub fn insert_argument_before(&mut self, index: usize,
        argument: &[u8]) -> Result<Event, Error> {
        let result = self.editor.insert_argument_before(index, argument)?;
        if result != Event::Empty {
            self.history_cursor = None;
            self.history_draft.fill(0);
            self.history_draft_len = 0;
        }
        Ok(result)
    }
    pub fn move_argument_to_end(&mut self, index: usize) -> Result<Event, Error> {
        let result = self.editor.move_argument_to_end(index)?;
        if result != Event::Empty {
            self.history_cursor = None;
            self.history_draft.fill(0);
            self.history_draft_len = 0;
        }
        Ok(result)
    }
}

#[cfg(test)]
mod r075_expanded_shell_edit_tests {
    use super::*;
    #[test]
    fn spacing_and_insert_preserve_mixed_case_paths_and_clipboard() {
        let mut input = InputLine::new();
        let mut command = [0u8; MAX_LINE];
        input.set_line(b"pwd").unwrap(); input.feed(b'\n', &mut command).unwrap();
        input.set_line(b"  git   status  /MiXeD  ").unwrap();
        input.copy_range(2, 5).unwrap();
        assert_eq!(input.compact_argument_spacing(), Ok(Event::Typed));
        assert_eq!(input.bytes(), b"git status /MiXeD");
        assert_eq!(input.insert_argument_before(1, b"log"), Ok(Event::Typed));
        assert_eq!(input.bytes(), b"git log status /MiXeD");
        assert_eq!(input.insert_argument_before(4, b"now"), Ok(Event::Typed));
        assert_eq!(input.bytes(), b"git log status /MiXeD now");
        assert_eq!(input.clipboard_bytes(), b"git");
        assert_eq!(input.history_count(), 1);
    }
    #[test]
    fn move_selected_argument_and_reject_invalid_index_or_invalid_input() {
        let mut input = InputLine::new();
        input.set_line(b"git status /Path").unwrap();
        assert_eq!(input.move_argument_to_end(1), Ok(Event::Typed));
        assert_eq!(input.bytes(), b"git /Path status");
        assert_eq!(input.move_argument_to_end(2), Ok(Event::Empty));
        let before = input.caret();
        assert_eq!(input.move_argument_to_end(7), Err(Error::Invalid));
        assert_eq!(input.insert_argument_before(9, b"next"), Err(Error::Invalid));
        assert_eq!(input.insert_argument_before(1, b"bad space"), Err(Error::Invalid));
        assert_eq!(input.bytes(), b"git /Path status");
        assert_eq!(input.caret(), before);
    }
}


// M1/R078 workstream 4 (tasks 13-16): word selection/editing in the
// session-owned live Terminal draft. No command is executed implicitly.
impl InputLine {
    /// Select an ASCII identifier adjacent to the caret. Spaces and shell
    /// punctuation are delimiters; no UTF-8 assumption or heap allocation.
    pub fn word_range_at_caret(&self)->Result<(usize,usize),Error>{
        let bytes=self.editor.bytes();
        let caret=self.editor.caret();
        if caret>bytes.len(){return Err(Error::Invalid);}
        let word=|b:u8|b.is_ascii_alphanumeric()||b==b'_';
        let at=if caret<bytes.len()&&word(bytes[caret]){
            caret
        }else if caret>0&&word(bytes[caret-1]){
            caret-1
        }else{return Err(Error::Invalid)};
        let mut start=at;let mut end=at+1;
        while start>0&&word(bytes[start-1]){start-=1;}
        while end<bytes.len()&&word(bytes[end]){end+=1;}
        Ok((start,end))
    }
    /// Copy the whole identifier under/just before the caret into the
    /// process-private clipboard. A miss keeps existing clipboard intact.
    pub fn copy_caret_word(&mut self)->Result<usize,Error>{
        let (a,b)=self.word_range_at_caret()?;
        self.copy_range(a,b)
    }
    /// Cut only the selected word; existing suffix and spaces are retained.
    pub fn cut_caret_word(&mut self)->Result<usize,Error>{
        let (a,b)=self.word_range_at_caret()?;
        self.cut_range(a,b)
    }
    /// Replace a command identifier without Enter, preserving history and
    /// clipboard. The existing atomic editor checks ASCII and capacity.
    pub fn replace_caret_word(&mut self,replacement:&[u8])->Result<Event,Error>{
        let (a,b)=self.word_range_at_caret()?;
        self.replace_range(a,b,replacement)
    }
}

#[cfg(test)]
mod r078_caret_word_tests {
    use super::*;
    #[test]
    fn word_copy_cut_paste_and_explicit_replacement_edit_live_draft(){
        let mut line=InputLine::new();
        line.set_line(b"run alpha_beta").unwrap();
        assert_eq!(line.word_range_at_caret(),Ok((4,14)));
        assert_eq!(line.copy_caret_word(),Ok(10));
        assert_eq!(line.clipboard_bytes(),b"alpha_beta");
        assert_eq!(line.replace_caret_word(b"code"),Ok(Event::Typed));
        assert_eq!(line.bytes(),b"run code");
        assert_eq!(line.cut_caret_word(),Ok(4));
        assert_eq!(line.bytes(),b"run ");
        assert_eq!(line.clipboard_bytes(),b"code");
        assert_eq!(line.paste_clipboard(),Ok(Event::Typed));
        assert_eq!(line.bytes(),b"run code");
        assert_eq!(line.history_count(),0);
    }
    #[test]
    fn missing_word_invalid_ascii_and_overlong_replacement_are_atomic(){
        let mut line=InputLine::new();
        line.set_line(b"build").unwrap();
        line.copy_caret_word().unwrap();
        assert_eq!(line.replace_caret_word(b"\n"),Err(Error::Invalid));
        assert_eq!(line.replace_caret_word(&[b'x';MAX_LINE+1]),Err(Error::Full));
        assert_eq!(line.bytes(),b"build");
        assert_eq!(line.clipboard_bytes(),b"build");
        line.set_line(b" ").unwrap();
        assert_eq!(line.copy_caret_word(),Err(Error::Invalid));
        assert_eq!(line.cut_caret_word(),Err(Error::Invalid));
        assert_eq!(line.replace_caret_word(b"oops"),Err(Error::Invalid));
        assert_eq!(line.bytes(),b" ");
        assert_eq!(line.clipboard_bytes(),b"build");
    }
}


// M1/R079 workstation 4: four non-submitting caret-word source transforms.
// All edits use the same bounded draft transaction and remain in the
// authenticated process, not terminal command execution or disk I/O.
impl InputLine {
    /// Uppercase only the ASCII identifier at the current caret.
    pub fn uppercase_caret_word(&mut self)->Result<Event,Error>{
        let (start,end)=self.word_range_at_caret()?;
        let mut next=[0u8;MAX_LINE];
        for (i,&b) in self.editor.bytes()[start..end].iter().enumerate(){
            next[i]=b.to_ascii_uppercase();
        }
        self.replace_range(start,end,&next[..end-start])
    }
    /// Lowercase only the ASCII identifier at the caret, not file paths,
    /// command history or adjacent arguments.
    pub fn lowercase_caret_word(&mut self)->Result<Event,Error>{
        let (start,end)=self.word_range_at_caret()?;
        let mut next=[0u8;MAX_LINE];
        for (i,&b) in self.editor.bytes()[start..end].iter().enumerate(){
            next[i]=b.to_ascii_lowercase();
        }
        self.replace_range(start,end,&next[..end-start])
    }
    /// Duplicate the selected identifier as id_id (one undo-neutral
    /// command-line edit); capacity is checked before touching the draft.
    pub fn duplicate_caret_word(&mut self)->Result<Event,Error>{
        let (start,end)=self.word_range_at_caret()?;
        let size=end-start;
        let total=size.checked_mul(2).and_then(|n|n.checked_add(1))
            .ok_or(Error::Full)?;
        if total>MAX_LINE{return Err(Error::Full);}
        let mut next=[0u8;MAX_LINE];
        next[..size].copy_from_slice(&self.editor.bytes()[start..end]);
        next[size]=b'_';
        next[size+1..total].copy_from_slice(&self.editor.bytes()[start..end]);
        self.replace_range(start,end,&next[..total])
    }
    /// Wrap the current identifier in user-selected printable delimiters
    /// such as quotes or braces while retaining all surrounding arguments.
    pub fn surround_caret_word(&mut self,prefix:&[u8],suffix:&[u8])
        ->Result<Event,Error>{
        let (start,end)=self.word_range_at_caret()?;
        let len=end-start;
        let total=prefix.len().checked_add(len)
            .and_then(|n|n.checked_add(suffix.len())).ok_or(Error::Full)?;
        if total>MAX_LINE{return Err(Error::Full);}
        if prefix.iter().chain(suffix.iter()).any(|&b|!(b' '..=b'~').contains(&b)){
            return Err(Error::Invalid);
        }
        let mut next=[0u8;MAX_LINE];
        next[..prefix.len()].copy_from_slice(prefix);
        next[prefix.len()..prefix.len()+len]
            .copy_from_slice(&self.editor.bytes()[start..end]);
        next[prefix.len()+len..total].copy_from_slice(suffix);
        self.replace_range(start,end,&next[..total])
    }
}
#[cfg(test)]
mod r079_caret_source_transforms{
    use super::*;
    #[test]
    fn uppercase_lowercase_clone_and_quote_preserve_other_arguments(){
        let mut line=InputLine::new();
        line.set_line(b"run alpha_beta").unwrap();
        assert_eq!(line.uppercase_caret_word(),Ok(Event::Typed));
        assert_eq!(line.bytes(),b"run ALPHA_BETA");
        assert_eq!(line.lowercase_caret_word(),Ok(Event::Typed));
        assert_eq!(line.bytes(),b"run alpha_beta");
        assert_eq!(line.duplicate_caret_word(),Ok(Event::Typed));
        assert_eq!(line.bytes(),b"run alpha_beta_alpha_beta");
        assert_eq!(line.surround_caret_word(b"(",b")"),Ok(Event::Typed));
        assert_eq!(line.bytes(),b"run (alpha_beta_alpha_beta)");
        assert_eq!(line.history_count(),0);
        assert!(line.clipboard_bytes().is_empty());
    }
    #[test]
    fn missing_word_invalid_delimiters_and_overlong_clones_never_edit(){
        let mut line=InputLine::new();
        line.set_line(b" ").unwrap();
        assert_eq!(line.uppercase_caret_word(),Err(Error::Invalid));
        assert_eq!(line.lowercase_caret_word(),Err(Error::Invalid));
        assert_eq!(line.duplicate_caret_word(),Err(Error::Invalid));
        line.set_line(b"alpha").unwrap();
        assert_eq!(line.surround_caret_word(b"\n",b")"),Err(Error::Invalid));
        assert_eq!(line.surround_caret_word(&[b'x';MAX_LINE],b""),Err(Error::Full));
        assert_eq!(line.bytes(),b"alpha");
        line.set_line(&[b'a';MAX_LINE]).unwrap();
        assert_eq!(line.duplicate_caret_word(),Err(Error::Full));
        assert_eq!(line.length(),MAX_LINE);
    }
}


// R080 workstream D: complete live command-argument manipulation using one
// bounded authenticated draft. Neither navigation nor editing executes it.
impl InputLine {
    /// The nth 0-based space-delimited argument; respects mixed-case paths.
    fn argument_bounds(&self,which:usize)->Result<(usize,usize),Error>{
        let bytes=self.editor.bytes();
        let mut at=0usize;let mut ordinal=0usize;
        while at<bytes.len(){
            while at<bytes.len()&&bytes[at]==b' '{at+=1;}
            if at==bytes.len(){break;}
            let begin=at;
            while at<bytes.len()&&bytes[at]!=b' '{at+=1;}
            if ordinal==which{return Ok((begin,at));}
            ordinal+=1;
        }
        Err(Error::Invalid)
    }
    /// Copy a token into the session-private clipboard only after successful
    /// range checks; an invalid ordinal never changes clipboard content.
    pub fn copy_argument(&mut self,which:usize)->Result<usize,Error>{
        let (from,to)=self.argument_bounds(which)?;
        self.copy_range(from,to)
    }
    /// Cut a token (with its adjacent spacing) using the existing
    /// delete_argument implementation, retaining the token in clipboard.
    pub fn cut_argument(&mut self,which:usize)->Result<usize,Error>{
        let (from,to)=self.argument_bounds(which)?;
        let len=to-from;
        let mut staged=[0u8;MAX_LINE];
        staged[..len].copy_from_slice(&self.editor.bytes()[from..to]);
        self.delete_argument(which)?;
        self.clipboard.fill(0);
        self.clipboard[..len].copy_from_slice(&staged[..len]);
        self.clipboard_len=len;
        Ok(len)
    }
    /// Surround one token with validated printable caller delimiters.
    /// The existing editor checks output capacity before any replacement.
    pub fn wrap_argument(&mut self,which:usize,left:&[u8],right:&[u8])
        ->Result<Event,Error>{
        let (from,to)=self.argument_bounds(which)?;
        if left.iter().chain(right.iter()).any(|b|!(b' '..=b'~').contains(b)){
            return Err(Error::Invalid);
        }
        let total=left.len().checked_add(to-from)
            .and_then(|n|n.checked_add(right.len())).ok_or(Error::Full)?;
        if total>MAX_LINE{return Err(Error::Full);}
        let mut staged=[0u8;MAX_LINE];
        staged[..left.len()].copy_from_slice(left);
        staged[left.len()..left.len()+to-from]
            .copy_from_slice(&self.editor.bytes()[from..to]);
        staged[left.len()+to-from..total].copy_from_slice(right);
        self.replace_range(from,to,&staged[..total])
    }
    /// Swap adjacent arguments, retaining exactly the original separators,
    /// with one atomic edit (and without touching command history).
    pub fn swap_next_argument(&mut self,which:usize)->Result<Event,Error>{
        let (a,b)=self.argument_bounds(which)?;
        let (c,d)=self.argument_bounds(which.checked_add(1).ok_or(Error::Invalid)?)?;
        if c<b{return Err(Error::Invalid);}
        let mut staged=[0u8;MAX_LINE];
        let mut used=0usize;
        for segment in [&self.editor.bytes()[c..d],
            &self.editor.bytes()[b..c],&self.editor.bytes()[a..b]]{
            staged[used..used+segment.len()].copy_from_slice(segment);
            used+=segment.len();
        }
        self.replace_range(a,d,&staged[..used])
    }
}
#[cfg(test)]
mod r080_input_argument_tests{
    use super::*;
    #[test]
    fn copy_cut_wrap_and_swap_are_real_unsent_draft_edits(){
        let mut input=InputLine::new();
        input.set_line(b"git  alpha  beta").unwrap();
        assert_eq!(input.copy_argument(1),Ok(5));
        assert_eq!(input.clipboard_bytes(),b"alpha");
        assert_eq!(input.swap_next_argument(1),Ok(Event::Typed));
        assert_eq!(input.bytes(),b"git  beta  alpha");
        assert_eq!(input.wrap_argument(1,b"(",b")"),Ok(Event::Typed));
        assert_eq!(input.bytes(),b"git  (beta)  alpha");
        assert_eq!(input.cut_argument(2),Ok(5));
        assert_eq!(input.bytes(),b"git  (beta)");
        assert_eq!(input.clipboard_bytes(),b"alpha");
        assert_eq!(input.history_count(),0);
    }
    #[test]
    fn malformed_tokens_bad_delimiters_and_capacity_are_atomic(){
        let mut input=InputLine::new();
        input.set_line(b"run alpha").unwrap();
        input.copy_argument(1).unwrap();
        assert_eq!(input.copy_argument(6),Err(Error::Invalid));
        assert_eq!(input.cut_argument(6),Err(Error::Invalid));
        assert_eq!(input.swap_next_argument(1),Err(Error::Invalid));
        assert_eq!(input.wrap_argument(1,b"\n",b")"),Err(Error::Invalid));
        assert_eq!(input.wrap_argument(1,&[b'(';MAX_LINE],b")"),Err(Error::Full));
        assert_eq!(input.bytes(),b"run alpha");
        assert_eq!(input.clipboard_bytes(),b"alpha");
    }
}


// M1/R081 workstream D: six complete argument-aware actions on the live
// keyboard draft. Every mutation is bounded and never implicitly submits.
impl InputLine{
    /// Number of nonempty ASCII-space delimited command tokens.
    pub fn argument_count(&self)->usize{
        let mut total=0usize;let mut inside=false;
        for &b in self.editor.bytes(){
            if b==b' '{inside=false;}
            else if !inside{total+=1;inside=true;}
        }
        total
    }
    /// Find an exact case-sensitive argument without exposing clipboard or
    /// scanning submitted history.
    pub fn find_argument_exact(&self,needle:&[u8])->Option<usize>{
        if needle.is_empty(){return None;}
        for idx in 0..self.argument_count(){
            let Ok((a,b))=self.argument_bounds(idx) else{continue};
            if &self.editor.bytes()[a..b]==needle{return Some(idx);}
        }
        None
    }
    /// Uppercase only a selected argument; existing editor enforces bounds.
    pub fn uppercase_argument(&mut self,index:usize)->Result<Event,Error>{
        let (a,b)=self.argument_bounds(index)?;
        let mut upper=[0u8;MAX_LINE];
        for (i,&v) in self.editor.bytes()[a..b].iter().enumerate(){
            upper[i]=v.to_ascii_uppercase();
        }
        self.replace_range(a,b,&upper[..b-a])
    }
    /// Lowercase only the selected token, retaining other flags/paths.
    pub fn lowercase_argument(&mut self,index:usize)->Result<Event,Error>{
        let (a,b)=self.argument_bounds(index)?;
        let mut lower=[0u8;MAX_LINE];
        for (i,&v) in self.editor.bytes()[a..b].iter().enumerate(){
            lower[i]=v.to_ascii_lowercase();
        }
        self.replace_range(a,b,&lower[..b-a])
    }
    /// Optimistic exact-token replacement, refusing a stale draft token.
    pub fn replace_argument_if_equal(&mut self,index:usize,expected:&[u8],
        new_value:&[u8])->Result<Event,Error>{
        let (a,b)=self.argument_bounds(index)?;
        if &self.editor.bytes()[a..b]!=expected{return Err(Error::Invalid);}
        self.replace_range(a,b,new_value)
    }
    /// Rotate shell arguments (excluding the verb) left by one. This explicit
    /// transform normalizes inter-argument separators to single spaces but
    /// leaves the command verb and surrounding prefix/suffix unchanged.
    pub fn rotate_arguments_left(&mut self)->Result<Event,Error>{
        let count=self.argument_count();
        if count<3{return Err(Error::Invalid);}
        let (first,_)=self.argument_bounds(1)?;
        let (_,last)=self.argument_bounds(count-1)?;
        let mut staged=[0u8;MAX_LINE];let mut used=0usize;
        for index in (2..count).chain(core::iter::once(1)){
            let (a,b)=self.argument_bounds(index)?;
            if used>0{staged[used]=b' ';used+=1;}
            staged[used..used+(b-a)].copy_from_slice(&self.editor.bytes()[a..b]);
            used+=b-a;
        }
        self.replace_range(first,last,&staged[..used])
    }
}
#[cfg(test)]
mod r081_live_argument_tests{
    use super::*;
    #[test]
    fn locate_case_change_cas_and_rotate_while_preserving_no_submission(){
        let mut input=InputLine::new();
        input.set_line(b"run  Alpha  beta GAMMA").unwrap();
        assert_eq!(input.argument_count(),4);
        assert_eq!(input.find_argument_exact(b"beta"),Some(2));
        assert_eq!(input.find_argument_exact(b"Beta"),None);
        assert_eq!(input.lowercase_argument(3),Ok(Event::Typed));
        assert_eq!(input.uppercase_argument(2),Ok(Event::Typed));
        assert_eq!(input.bytes(),b"run  Alpha  BETA gamma");
        assert_eq!(input.replace_argument_if_equal(1,b"Alpha",b"first"),Ok(Event::Typed));
        assert_eq!(input.rotate_arguments_left(),Ok(Event::Typed));
        assert_eq!(input.bytes(),b"run  BETA gamma first");
        assert_eq!(input.history_count(),0);
        assert!(input.clipboard_bytes().is_empty());
    }
    #[test]
    fn invalid_ordinals_and_stale_text_leave_draft_unchanged(){
        let mut input=InputLine::new();input.set_line(b"cmd one").unwrap();
        assert_eq!(input.rotate_arguments_left(),Err(Error::Invalid));
        assert_eq!(input.uppercase_argument(5),Err(Error::Invalid));
        assert_eq!(input.lowercase_argument(5),Err(Error::Invalid));
        assert_eq!(input.replace_argument_if_equal(1,b"two",b"other"),
            Err(Error::Invalid));
        assert_eq!(input.bytes(),b"cmd one");
        assert_eq!(input.argument_count(),2);
        assert_eq!(input.find_argument_exact(b""),None);
    }
}
