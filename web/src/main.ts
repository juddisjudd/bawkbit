import { mount } from 'svelte'
import './fonts.css'
import './app.css'
import App from './App.svelte'

mount(App, { target: document.getElementById('app')! })
