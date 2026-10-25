//! Batch 5 task 25: first India-focused English/Hindi interface strings
//! and accessible interaction preflight. Actual text rasterization, fonts,
//! spoken output and desktop events must be attached to ring-3 GUI services.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]pub enum Locale{English,Hindi}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]pub enum TextId{
 Desktop,Files,Terminal,Settings,Install,Cancel,Allow,Deny,
}
pub fn text(locale:Locale,key:TextId)->&'static str{
 match (locale,key){
  (Locale::English,TextId::Desktop)=>"Desktop",
  (Locale::English,TextId::Files)=>"Files",
  (Locale::English,TextId::Terminal)=>"Terminal",
  (Locale::English,TextId::Settings)=>"Settings",
  (Locale::English,TextId::Install)=>"Install",
  (Locale::English,TextId::Cancel)=>"Cancel",
  (Locale::English,TextId::Allow)=>"Allow",
  (Locale::English,TextId::Deny)=>"Deny",
  (Locale::Hindi,TextId::Desktop)=>"डेस्कटॉप",
  (Locale::Hindi,TextId::Files)=>"फ़ाइलें",
  (Locale::Hindi,TextId::Terminal)=>"टर्मिनल",
  (Locale::Hindi,TextId::Settings)=>"सेटिंग्स",
  (Locale::Hindi,TextId::Install)=>"इंस्टॉल करें",
  (Locale::Hindi,TextId::Cancel)=>"रद्द करें",
  (Locale::Hindi,TextId::Allow)=>"अनुमति दें",
  (Locale::Hindi,TextId::Deny)=>"अस्वीकार करें",
 }
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]pub enum AccessibilityError{
 SmallTarget,NoKeyboardAccess,NoLabel
}
/// Testable policy for focusable installer/desktop controls.
/// This does not render or inspect an actual GUI.
pub fn check_control(width:u32,height:u32,label:&str,
  keyboard_activatable:bool)->Result<(),AccessibilityError>{
 if width<36||height<36{return Err(AccessibilityError::SmallTarget);}
 if !keyboard_activatable{return Err(AccessibilityError::NoKeyboardAccess);}
 if label.trim().is_empty(){return Err(AccessibilityError::NoLabel);}
 Ok(())
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn hindi_and_english_prompts_have_visible_names(){
  for locale in [Locale::English,Locale::Hindi]{
   for id in [TextId::Desktop,TextId::Files,TextId::Terminal,
    TextId::Settings,TextId::Install,TextId::Cancel,TextId::Allow,TextId::Deny]{
    assert!(!text(locale,id).is_empty());
   }
  }
 }
 #[test]fn installer_controls_need_keyboard_and_hit_target(){
  assert_eq!(check_control(35,40,"Install",true),Err(AccessibilityError::SmallTarget));
  assert_eq!(check_control(40,40,"Install",false),Err(AccessibilityError::NoKeyboardAccess));
  assert_eq!(check_control(40,40," ",true),Err(AccessibilityError::NoLabel));
  assert_eq!(check_control(40,40,text(Locale::Hindi,TextId::Install),true),Ok(()));
 }
}