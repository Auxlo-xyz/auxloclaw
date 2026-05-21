import React, { useState, useEffect } from 'react';
import { Plus, Trash2, Search, Edit3, Save, X } from 'lucide-react';
import { motion, AnimatePresence } from 'framer-motion';

const App = () => {
  const [notes, setNotes] = useState([]);
  const [isEditing, setIsEditing] = useState(false);
  const [searchQuery, setSearchQuery] = useState('');
  const [currentNote, setCurrentNote] = useState({ title: '', content: '', color: 'bg-indigo-100' });

  useEffect(() => {
    const savedNotes = localStorage.getItem('modern-notes');
    if (savedNotes) {
      setNotes(JSON.parse(savedNotes));
    }
  }, []);

  useEffect(() => {
    localStorage.setItem('modern-notes', JSON.stringify(notes));
  }, [notes]);

  const handleAddNote = () => {
    setIsEditing(true);
    setCurrentNote({ title: '', content: '', color: 'bg-indigo-100' });
  };

  const handleSaveNote = () => {
    if (currentNote.title.trim() === '' && currentNote.content.trim() === '') return;
    
    if (currentNote.id) {
      setNotes(notes.map(n => n.id === currentNote.id ? currentNote : n));
    } else {
      const newNote = { ...currentNote, id: Date.now() };
      setNotes([newNote, ...notes]);
    }
    setIsEditing(false);
  };

  const handleDeleteNote = (id) => {
    setNotes(notes.filter(n => n.id !== id));
  };

  const handleEditNote = (note) => {
    setCurrentNote(note);
    setIsEditing(true);
  };

  const filteredNotes = notes.filter(note => 
    note.title.toLowerCase().includes(searchQuery.toLowerCase()) || 
    note.content.toLowerCase().includes(searchQuery.toLowerCase())
  );

  const colors = [
    'bg-indigo-100 border-indigo-200 text-indigo-800',
    'bg-rose-100 border-rose-200 text-rose-800',
    'bg-amber-100 border-amber-200 text-amber-800',
    'bg-emerald-100 border-emerald-200 text-emerald-800',
    'bg-blue-100 border-blue-200 text-blue-800',
    'bg-purple-100 border-purple-200 text-purple-800',
  ];

  return (
    <div className="min-h-screen p-4 md:p-8 lg:p-12 max-w-6xl mx-auto">
      {/* Header */}
      <header className="flex flex-col md:flex-row md:items-center justify-between gap-4 mb-12">
        <div>
          <h1 className="text-4xl font-bold bg-clip-text text-transparent bg-gradient-to-r from-indigo-600 to-purple-600">
            Modern Notes
          </h1>
          <p className="text-slate-500 mt-1">Capture your thoughts instantly.</p>
        </div>
        
        <div className="flex items-center gap-3">
          <div className="relative group">
            <Search className="absolute left-3 top-1/2 -translate-y-1/2 text-slate-400 group-focus-within:text-indigo-500 transition-colors" size={18} />
            <input 
              type="text" 
              placeholder="Search notes..." 
              className="pl-10 pr-4 py-2 bg-white border border-slate-200 rounded-xl focus:outline-none focus:ring-2 focus:ring-indigo-500/20 focus:border-indigo-500 transition-all w-full md:w-64"
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
            />
          </div>
          <button 
            onClick={handleAddNote}
            className="bg-indigo-600 hover:bg-indigo-700 text-white p-2 rounded-xl transition-all shadow-lg shadow-indigo-200 active:scale-95"
          >
            <Plus size={24} />
          </button>
        </div>
      </header>

      {/* Notes Grid */}
      <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-6">
        <AnimatePresence>
          {filteredNotes.map((note) => (
            <motion.div 
              layout
              initial={{ opacity: 0, scale: 0.9 }}
              animate={{ opacity: 1, scale: 1 }}
              exit={{ opacity: 0, scale: 0.9 }}
              whileHover={{ y: -5 }}
              key={note.id}
              className={`p-6 rounded-2xl border-2 ${note.color || 'bg-indigo-50 border-indigo-100 text-indigo-900'} shadow-sm transition-all cursor-pointer group`}
              onClick={() => handleEditNote(note)}
            >
              <div className="flex justify-between items-start mb-3">
                <h3 className="font-bold text-xl truncate">{note.title || 'Untitled Note'}</h3>
                <button 
                  onClick={(e) => {
                    e.stopPropagation();
                    handleDeleteNote(note.id);
                  }}
                  className="opacity-0 group-hover:opacity-100 p-1.5 hover:bg-white/50 rounded-lg transition-all text-slate-500 hover:text-rose-600"
                >
                  <Trash2 size={16} />
                </button>
              </div>
              <p className="text-sm leading-relaxed line-clamp-4 opacity-80">{note.content}</p>
            </motion.div>
          ))}
        </AnimatePresence>
      </div>

      {/* Empty State */}
      {filteredNotes.length === 0 && (
        <div className="flex flex-col items-center justify-center py-20 text-center">
          <div className="w-20 h-20 bg-indigo-50 text-indigo-500 rounded-full flex items-center justify-center mb-4">
            <Edit3 size={32} />
          </div>
          <h2 className="text-xl font-semibold text-slate-800">No notes found</h2>
          <p className="text-slate-500">Start by creating your first note!</p>
        </div>
      )}

      {/* Note Editor Modal */}
      <AnimatePresence>
        {isEditing && (
          <motion.div 
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-slate-900/40 backdrop-blur-sm"
          >
            <motion.div 
              initial={{ scale: 0.9, opacity: 0, y: 20 }}
              animate={{ scale: 1, opacity: 1, y: 0 }}
              exit={{ scale: 0.9, opacity: 0, y: 20 }}
              className="bg-white w-full max-w-lg rounded-3xl shadow-2xl overflow-hidden"
            >
              <div className="p-6 flex items-center justify-between border-b border-slate-100">
                <h2 className="text-xl font-bold text-slate-800">Edit Note</h2>
                <button 
                  onClick={() => setIsEditing(false)}
                  className="p-2 hover:bg-slate-100 rounded-full transition-colors text-slate-400"
                >
                  <X size={20} />
                </button>
              </div>
              
              <div className="p-6 space-y-4">
                <input 
                  type="text" 
                  placeholder="Title" 
                  className="w-full text-2xl font-bold focus:outline-none placeholder:text-slate-300"
                  value={currentNote.title}
                  onChange={(e) => setCurrentNote({ ...currentNote, title: e.target.value })}
                  autoFocus
                />
                <textarea 
                  placeholder="Write your thoughts here..." 
                  className="w-full h-64 resize-none focus:outline-none placeholder:text-slate-300 leading-relaxed"
                  value={currentNote.content}
                  onChange={(e) => setCurrentNote({ ...currentNote, content: e.target.value })}
                />
                
                <div className="flex items-center gap-3 pt-4">
                  {colors.map((color, idx) => (
                    <button 
                      key={idx} 
                      onClick={() => setCurrentNote({ ...currentNote, color: color })}
                      className={`w-8 h-8 rounded-full border-2 transition-all ${color} ${currentNote.color === color ? 'ring-2 ring-offset-2 ring-indigo-500' : ''}`}
                    />
                  ))}
                </div>
              </div>
              
              <div className="p-6 bg-slate-50 flex justify-end gap-3">
                <button 
                  onClick={() => setIsEditing(false)}
                  className="px-4 py-2 text-slate-600 hover:bg-slate-200 rounded-xl transition-colors"
                >
                  Cancel
                </button>
                <button 
                  onClick={handleSaveNote}
                  className="px-6 py-2 bg-indigo-600 text-white rounded-xl hover:bg-indigo-700 transition-colors shadow-lg shadow-indigo-200 flex items-center gap-2"
                >
                  <Save size={18} />
                  Save Note
                </button>
              </div>
            </motion.div>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
};

export default App;
